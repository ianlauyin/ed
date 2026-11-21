//! Serves `edctl` requests (see the `control` crate) when automation is enabled.

use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context as _;
use anyhow::Result;
use async_macos::net::UnixListener;
use async_macos::net::UnixStream;
use control::Command;
use control::MouseCommand;
use control::MouseKind;
use control::Request;
use control::Response;
use editor::Editor;
use futures::io::AsyncBufReadExt as _;
use futures::io::AsyncWriteExt as _;
use futures::io::BufReader;
use gpui::AnyWindowHandle;
use gpui::App;
use gpui::AsyncApp;
use gpui::Bounds;
use gpui::CursorHideMode;
use gpui::DurationSummary;
use gpui::ElementId;
use gpui::Entity;
use gpui::Focusable as _;
use gpui::FrameStatsSummary;
use gpui::Keystroke;
use gpui::Modifiers;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::MouseMoveEvent;
use gpui::MouseUpEvent;
use gpui::Pixels;
use gpui::PlatformInput;
use gpui::ScrollDelta;
use gpui::ScrollWheelEvent;
use gpui::TouchPhase;
use gpui::Window;
use gpui::point;
use gpui::px;
use language::Point;
use rustc_hash::FxHashMap;
use serde_json::Value;
use util::ResultExt as _;
use workspace::Pane;
use workspace::Workspace;
use workspace::WorkspaceMode;
use workspace::dock::DockPosition;
use workspace::item::ItemHandle;

use crate::ed::cpu_usage::CpuSample;

const SETTLE_TIMEOUT: Duration = Duration::from_secs(1);
const SETTLE_TICK: Duration = Duration::from_millis(16);
/// Consecutive clean ticks before a window counts as settled.
const SETTLE_QUIET_TICKS: usize = 3;

/// The mouse button held by a `mouse down` without a matching `mouse up`, shared across connections.
type PressedButton = Rc<Cell<Option<MouseButton>>>;
/// CPU counters at the last `perf_reset`, shared across connections.
type PerfStart = Rc<Cell<Option<CpuSample>>>;

pub(crate) fn init(cx: &mut App) {
    let Some(socket_path) = control::socket_path_from_env() else {
        return;
    };
    let listener = match bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("failed to listen on control socket {}: {error:#}", socket_path.display());
            return;
        }
    };
    log::info!("control socket listening on {}", socket_path.display());
    cx.enable_ui_snapshots();

    cx.on_app_quit(move |_| {
        fs::remove_file(&socket_path).ok();
        async {}
    })
    .detach();

    let pressed_button = PressedButton::default();
    let perf_start = PerfStart::default();
    cx.spawn(async move |cx| {
        loop {
            match listener.accept().await {
                Ok(stream) => {
                    let pressed_button = Rc::clone(&pressed_button);
                    let perf_start = Rc::clone(&perf_start);
                    cx.spawn(async move |cx| handle_connection(stream, pressed_button, perf_start, cx).await.log_err())
                        .detach();
                }
                Err(error) => {
                    log::error!("control socket stopped accepting connections: {error}");
                    break;
                }
            }
        }
    })
    .detach();
}

fn bind(socket_path: &Path) -> Result<UnixListener> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::remove_file(socket_path).ok();
    Ok(UnixListener::bind(socket_path)?)
}

async fn handle_connection(
    stream: UnixStream,
    pressed_button: PressedButton,
    perf_start: PerfStart,
    cx: &mut AsyncApp,
) -> Result<()> {
    let mut reader = BufReader::new(stream.clone());
    let mut writer = stream;
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        let request = serde_json::from_str::<Request>(&line);
        line.clear();
        let quit = matches!(request, Ok(Request { command: Command::Quit, .. }));
        let response = match request {
            Ok(request) => match handle_request(request, &pressed_button, &perf_start, cx).await {
                Ok(value) => Response::Ok(value),
                Err(error) => Response::Error(format!("{error:#}")),
            },
            Err(error) => Response::Error(format!("invalid request: {error}")),
        };
        let mut response = serde_json::to_string(&response)?;
        response.push('\n');
        writer.write_all(response.as_bytes()).await?;
        writer.flush().await?;
        if quit {
            cx.update(|cx| cx.quit());
        }
    }
    Ok(())
}

async fn handle_request(
    request: Request,
    pressed_button: &PressedButton,
    perf_start: &PerfStart,
    cx: &mut AsyncApp,
) -> Result<Value> {
    match request.command {
        Command::Windows => return windows(cx),
        Command::Quit => return Ok(Value::Null),
        _ => {}
    }

    let window = cx.update(|cx| target_window(request.window, cx))?;
    let value = match request.command {
        Command::Windows | Command::Quit => unreachable!("handled above"),
        Command::Tree => window.update(cx, |_, window, cx| {
            window.draw_if_dirty(cx);
            serde_json::to_value(tree(window))
        })??,
        Command::State => window.update(cx, |_, window, cx| serde_json::to_value(state(window, cx)))??,
        Command::Actions { available } => {
            let mut names: Vec<String> = window.update(cx, |_, window, cx| {
                if available {
                    window.available_actions(cx).iter().map(|action| action.name().to_owned()).collect()
                } else {
                    cx.all_action_names().iter().map(ToString::to_string).collect()
                }
            })?;
            names.sort();
            names.dedup();
            serde_json::to_value(names)?
        }
        Command::Action { name, args } => {
            window.update(cx, |_, window, cx| -> Result<()> {
                let action = cx.build_action(&name, args).map_err(|error| anyhow::anyhow!("{error}"))?;
                window.dispatch_action(action, cx);
                Ok(())
            })??;
            settle(window, cx).await?;
            Value::Null
        }
        Command::Keys { keystrokes } => {
            let keystrokes = keystrokes
                .split_whitespace()
                .map(|keystroke| {
                    Keystroke::parse(keystroke).with_context(|| format!("invalid keystroke {keystroke:?}"))
                })
                .collect::<Result<Vec<Keystroke>>>()?;
            for keystroke in keystrokes {
                dispatch_keystroke(window, keystroke, cx)?;
            }
            settle(window, cx).await?;
            Value::Null
        }
        Command::Type { text } => {
            for character in text.chars() {
                dispatch_keystroke(window, keystroke_for_char(character), cx)?;
            }
            settle(window, cx).await?;
            Value::Null
        }
        Command::Mouse(mouse) => {
            dispatch_mouse(window, &mouse, pressed_button, cx)?;
            settle(window, cx).await?;
            Value::Null
        }
        Command::Scroll { x, y, dx, dy } => {
            let position = point(px(x), px(y));
            dispatch_event(
                window,
                PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }),
                cx,
            )?;
            dispatch_event(
                window,
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position,
                    delta: ScrollDelta::Pixels(point(px(-dx), px(-dy))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            )?;
            settle(window, cx).await?;
            Value::Null
        }
        Command::Screenshot { path, no_settle } => {
            if !no_settle {
                settle(window, cx).await?;
            }
            let image = window.update(cx, |_, window, _| window.capture_image())??;
            image.save(&path).with_context(|| format!("failed to write {}", path.display()))?;
            Value::Null
        }
        Command::Settle => {
            settle(window, cx).await?;
            Value::Null
        }
        Command::PerfReset => {
            window.update(cx, |_, window, _| window.reset_frame_stats())?;
            perf_start.set(Some(CpuSample::now()));
            Value::Null
        }
        Command::Perf => {
            let start = perf_start.get().context("call perf_reset first")?;
            let cpu = CpuSample::now().since(&start);
            let frames = window.update(cx, |_, window, _| {
                window.frame_stats().map_or_default(|stats| frame_times(&stats.summary()))
            })?;
            serde_json::to_value(control::Perf { cpu, frames })?
        }
    };
    Ok(value)
}

fn frame_times(summary: &FrameStatsSummary) -> control::FrameTimes {
    let durations = |durations: &DurationSummary| control::Durations {
        total_ms: durations.total_ms,
        mean_ms: durations.mean_ms,
        p50_ms: durations.p50_ms,
        p95_ms: durations.p95_ms,
        max_ms: durations.max_ms,
    };
    control::FrameTimes {
        frames: summary.frames,
        presents: summary.presents,
        draw: durations(&summary.draw),
        build: durations(&summary.build),
        paint: durations(&summary.paint),
        present: durations(&summary.present),
        shaped_lines: summary.shaped_lines.total,
        shaped_lines_max: summary.shaped_lines.max,
    }
}

fn target_window(id: Option<u64>, cx: &App) -> Result<AnyWindowHandle> {
    let windows = cx.windows();
    match id {
        Some(id) => {
            windows.into_iter().find(|window| window.window_id().as_u64() == id).context("no window with that id")
        }
        None => cx.active_window().or_else(|| windows.first().copied()).context("the app has no windows"),
    }
}

fn windows(cx: &AsyncApp) -> Result<Value> {
    let windows = cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|handle| {
                handle
                    .update(cx, |_, window, _| control::WindowInfo {
                        id: handle.window_id().as_u64(),
                        title: window.window_title(),
                        active: window.is_window_active(),
                        bounds: rect(window.bounds()),
                        scale_factor: window.scale_factor(),
                    })
                    .ok()
            })
            .collect::<Vec<control::WindowInfo>>()
    });
    Ok(serde_json::to_value(windows)?)
}

/// Redraw until the window stays clean for a few ticks, so follow-up reads see the
/// result. A pending animation frame counts as not clean; the platform draws it.
async fn settle(window: AnyWindowHandle, cx: &mut AsyncApp) -> Result<()> {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    let mut quiet_ticks = 0;
    while quiet_ticks < SETTLE_QUIET_TICKS && Instant::now() < deadline {
        let dirty = window.update(cx, |_, window, cx| {
            let dirty = window.is_dirty();
            window.draw_if_dirty(cx);
            dirty || window.wants_next_frame()
        })?;
        quiet_ticks = if dirty { 0 } else { quiet_ticks + 1 };
        cx.background_executor().timer(SETTLE_TICK).await;
    }
    Ok(())
}

/// Dispatch in its own update, then draw, so the next event sees focus and key
/// contexts changed by this one. Typing doesn't hide the cursor: only a real
/// mouse move shows it again, so injected mouse input would see it hidden.
fn dispatch_keystroke(window: AnyWindowHandle, keystroke: Keystroke, cx: &mut AsyncApp) -> Result<()> {
    window.update(cx, |_, window, cx| {
        let cursor_hide_mode = cx.cursor_hide_mode();
        cx.set_cursor_hide_mode(CursorHideMode::Never);
        window.dispatch_keystroke(keystroke, cx);
        cx.set_cursor_hide_mode(cursor_hide_mode);
    })?;
    window.update(cx, |_, window, cx| window.draw_if_dirty(cx))
}

fn dispatch_event(window: AnyWindowHandle, event: PlatformInput, cx: &mut AsyncApp) -> Result<()> {
    window.update(cx, |_, window, cx| {
        window.dispatch_event(event, cx);
    })?;
    window.update(cx, |_, window, cx| window.draw_if_dirty(cx))
}

fn keystroke_for_char(character: char) -> Keystroke {
    let (key, key_char) = match character {
        ' ' => ("space".to_owned(), " ".to_owned()),
        '\n' => ("enter".to_owned(), "\n".to_owned()),
        '\t' => ("tab".to_owned(), "\t".to_owned()),
        character => (character.to_lowercase().to_string(), character.to_string()),
    };
    let modifiers = Modifiers { shift: character.is_uppercase(), ..Modifiers::default() };
    Keystroke { modifiers, key, key_char: Some(key_char) }
}

fn dispatch_mouse(
    window: AnyWindowHandle,
    mouse: &MouseCommand,
    pressed_button: &PressedButton,
    cx: &mut AsyncApp,
) -> Result<()> {
    let position = point(px(mouse.x), px(mouse.y));
    let button = match mouse.button {
        control::MouseButton::Left => MouseButton::Left,
        control::MouseButton::Right => MouseButton::Right,
        control::MouseButton::Middle => MouseButton::Middle,
    };
    let modifiers = Modifiers {
        control: mouse.modifiers.ctrl,
        alt: mouse.modifiers.alt,
        shift: mouse.modifiers.shift,
        platform: mouse.modifiers.cmd,
        function: false,
    };
    let move_event =
        PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: pressed_button.get(), modifiers });
    let down_event = PlatformInput::MouseDown(MouseDownEvent {
        button,
        position,
        modifiers,
        click_count: mouse.click_count,
        first_mouse: false,
    });
    let up_event = PlatformInput::MouseUp(MouseUpEvent { button, position, modifiers, click_count: mouse.click_count });

    match mouse.kind {
        MouseKind::Move => dispatch_event(window, move_event, cx),
        MouseKind::Down => {
            dispatch_event(window, move_event, cx)?;
            pressed_button.set(Some(button));
            dispatch_event(window, down_event, cx)
        }
        MouseKind::Up => {
            dispatch_event(window, move_event, cx)?;
            pressed_button.set(None);
            dispatch_event(window, up_event, cx)
        }
        MouseKind::Click => {
            dispatch_event(window, move_event, cx)?;
            dispatch_event(window, down_event, cx)?;
            dispatch_event(window, up_event, cx)
        }
    }
}

fn tree(window: &Window) -> control::Tree {
    let focus = window.focus_id();
    let ui_elements = window.ui_elements();
    let index_by_id: FxHashMap<&[ElementId], usize> =
        ui_elements.iter().enumerate().map(|(ix, element)| ((&*element.id), ix)).collect();

    let mut elements: Vec<control::Element> = ui_elements
        .iter()
        .map(|element| control::Element {
            id: format_id(&element.id),
            bounds: rect(element.bounds),
            clickable: element.clickable,
            focusable: element.focus.is_some(),
            focused: element.focus.is_some() && element.focus == focus,
            texts: Vec::new(),
        })
        .collect();
    let mut texts = Vec::new();
    for ui_text in window.ui_texts() {
        let text = control::Text { text: ui_text.text.to_string(), bounds: rect(ui_text.bounds) };
        let owner = (0..=ui_text.id.len()).rev().find_map(|len| index_by_id.get(&ui_text.id[..len]).copied());
        match owner {
            Some(ix) => elements[ix].texts.push(text),
            None => texts.push(text),
        }
    }

    control::Tree {
        window: window.window_handle().window_id().as_u64(),
        focused: elements.iter().find(|element| element.focused).map(|element| element.id.clone()),
        elements,
        texts,
    }
}

/// Element ids joined by `/`, without view and focus-handle ids, which change between runs.
fn format_id(ids: &[ElementId]) -> String {
    ids.iter()
        .filter(|id| !matches!(id, ElementId::View(_) | ElementId::FocusHandle(_)))
        .map(|id| {
            // Component ids are Rust type paths; the last segment is enough.
            let id = id.to_string();
            match id.rsplit_once("::") {
                Some((_, name)) if !id.contains('/') => name.to_owned(),
                _ => id,
            }
        })
        .collect::<Vec<String>>()
        .join("/")
}

const fn rect(bounds: Bounds<Pixels>) -> control::Rect {
    control::Rect {
        x: bounds.origin.x.as_f32(),
        y: bounds.origin.y.as_f32(),
        width: bounds.size.width.as_f32(),
        height: bounds.size.height.as_f32(),
    }
}

fn state(window: &Window, cx: &mut App) -> control::State {
    let window_id = window.window_handle().window_id().as_u64();
    let bounds = rect(window.bounds());
    let display_visible_bounds = window.display(cx).map(|display| rect(display.visible_bounds()));
    let workspace = window.root::<Workspace>().flatten();
    let Some(workspace) = workspace else {
        return control::State { window: window_id, bounds, display_visible_bounds, workspace: None };
    };

    let focused_pane = workspace.read(cx).focused_pane(window, cx);
    let workspace = workspace.read(cx);
    let project = workspace.project().read(cx);
    let item_path = |item: &dyn ItemHandle| {
        let project_path = item.project_path(cx)?;
        project.absolute_path(&project_path, cx)
    };

    let pane_state = |pane_entity: &Entity<Pane>, active_pane: &Entity<Pane>| {
        let pane = pane_entity.read(cx);
        let items: Vec<control::ItemState> = pane
            .items()
            .map(|item| control::ItemState {
                title: item.tab_content_text(0, cx).to_string(),
                path: item_path(item.as_ref()),
                dirty: item.is_dirty(cx),
            })
            .collect();
        control::PaneState {
            focused: *pane_entity == focused_pane,
            active: pane_entity == active_pane,
            has_focus: pane.has_focus(window, cx),
            active_item: (!items.is_empty()).then(|| pane.active_item_index()),
            items,
        }
    };
    let panes = workspace.panes().iter().map(|pane| pane_state(pane, workspace.active_pane())).collect();
    let git_history_panes =
        workspace.git_history_panes().into_iter().map(|pane| pane_state(pane, workspace.git_history_pane())).collect();
    let terminal_panes =
        workspace.terminal_panes().into_iter().map(|pane| pane_state(pane, workspace.terminal_pane())).collect();
    let docks = workspace
        .all_docks()
        .into_iter()
        .map(|dock_entity| {
            let dock = dock_entity.read(cx);
            control::DockState {
                position: dock_position_name(dock.position()).to_owned(),
                open: dock.is_open(),
                visible_panel: dock.visible_panel().map(|panel| panel.persistent_name().to_owned()),
                has_focus: dock_entity.focus_handle(cx).contains_focused(window, cx),
            }
        })
        .collect();
    let mode = match workspace.mode() {
        WorkspaceMode::Editor => "editor",
        WorkspaceMode::GitHistory => "git_history",
        WorkspaceMode::Terminal => "terminal",
    }
    .to_owned();

    let roots = workspace.root_paths(cx).iter().map(|path| path.to_path_buf()).collect();
    let language_servers = workspace
        .project()
        .read(cx)
        .language_server_statuses(cx)
        .map(|(_, status)| control::LanguageServerState {
            name: status.name.to_string(),
            work: status
                .pending_work
                .values()
                .map(|progress| {
                    [progress.title.as_deref(), progress.message.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(": ")
                })
                .collect(),
        })
        .collect();

    let active_item = workspace.active_item(cx);
    let active_path = active_item.as_ref().and_then(|item| item_path(item.as_ref()));
    let active_editor = active_item.and_then(|item| item.act_as::<Editor>(cx));

    let active_editor = active_editor.map(|editor| {
        editor.update(cx, |editor, cx| {
            let snapshot = editor.display_snapshot(cx);
            let selections = editor
                .selections
                .all::<Point>(&snapshot)
                .into_iter()
                .map(|selection| control::Selection {
                    start: control::Position { row: selection.start.row, column: selection.start.column },
                    end: control::Position { row: selection.end.row, column: selection.end.column },
                    reversed: selection.reversed,
                })
                .collect();
            control::EditorState { path: active_path, text: editor.text(cx), selections }
        })
    });

    control::State {
        window: window_id,
        bounds,
        display_visible_bounds,
        workspace: Some(control::WorkspaceState {
            roots,
            mode,
            panes,
            git_history_panes,
            terminal_panes,
            docks,
            active_editor,
            language_servers,
        }),
    }
}

const fn dock_position_name(position: DockPosition) -> &'static str {
    match position {
        DockPosition::Left => "left",
        DockPosition::Bottom => "bottom",
    }
}

#[cfg(test)]
mod tests {
    use workspace::dock::DockPosition;

    use super::dock_position_name;

    #[test]
    fn dock_position_name_works() {
        assert_eq!(dock_position_name(DockPosition::Left), "left");
        assert_eq!(dock_position_name(DockPosition::Bottom), "bottom");
    }
}
