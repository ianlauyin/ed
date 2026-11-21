use diagnostics::items::DiagnosticIndicator;
use ed_actions::project_panel::ToggleFocus;
use go_to_line::cursor_position::CursorPosition;
use search::search_status_button::SearchButton;
use ui::font;
use ui::utils::TRAFFIC_LIGHT_POSITION;
mod app_menus;
pub(crate) mod control_server;
pub(crate) mod cpu_usage;
mod keymap;

pub(crate) mod mac_only_instance;

pub(crate) mod open_listener;

use std::borrow::Cow;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context as _;
use app_menus::app_menus;
use ed_actions::About;
use ed_actions::OpenBrowser;
use ed_actions::Quit;
use editor::Editor;
use editor::MultiBuffer;
use git_ui::branch_diff::BranchDiffToolbar;
use git_ui::commit_view::CommitViewToolbar;
use git_ui::git_panel::GitPanel;
use git_ui::project_diff::ProjectDiffToolbar;
use git_ui::solo_diff_view::SoloDiffGitToolbar;
use git_ui::solo_diff_view::SoloDiffStyleToolbar;
use git_ui::staged_diff::StagedDiffToolbar;
use git_ui::unstaged_diff::UnstagedDiffToolbar;
use git_ui_core::file_diff_view::FileDiffStyleToolbar;
use gpui::App;
use gpui::AppContext as _;
use gpui::Bounds;
use gpui::Context;
use gpui::Entity;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::Image;
use gpui::ImageFormat;
use gpui::ParentElement as _;
use gpui::PathPromptOptions;
use gpui::PlatformDisplay;
use gpui::SharedString;
use gpui::Size;
use gpui::Task;
use gpui::TaskExt as _;
use gpui::TitlebarOptions;
use gpui::WeakEntity;
use gpui::Window;
use gpui::WindowBounds;
use gpui::WindowHandle;
use gpui::WindowKind;
use gpui::WindowOptions;
use gpui::actions;
use gpui::img;
use gpui::point;
use gpui::px;
use image_viewer::ImageInfo;
use language::Capability;
use language_tools::lsp_button;
use language_tools::lsp_button::LspButton;
use language_tools::lsp_log_view::LspLogToolbarItemView;
use project_panel::ProjectPanel;
use search::project_search::ProjectSearchBar;
use terminal_view::terminal_panel;
use terminal_view::terminal_panel::TerminalPanel;
use theme::ActiveTheme as _;
use ui::PopoverMenuHandle;
use ui::prelude::Button;
use ui::prelude::ButtonCommon as _;
use ui::prelude::ButtonStyle;
use ui::prelude::Clickable as _;
use ui::prelude::FixedWidth as _;
use ui::prelude::Headline;
use ui::prelude::InteractiveElement as _;
use ui::prelude::IntoElement;
use ui::prelude::Render;
use ui::prelude::Styled as _;
use ui::prelude::VisualContext as _;
use ui::prelude::v_flex;
use util::ResultExt as _;
use uuid::Uuid;
use workspace::AppState;
use workspace::CloseIntent;
use workspace::CloseProject;
use workspace::CloseWindow;
use workspace::NewFile;
use workspace::NewWindow;
use workspace::OpenLog;
use workspace::Pane;
use workspace::Workspace;
use workspace::notifications::NotificationId;
use workspace::notifications::simple_message_notification::MessageNotification;
use workspace::open_new;
use workspace::with_active_or_new_workspace;

actions!(
    ed,
    [
        /// Hides the application window.
        Hide,
        /// Hides all other application windows.
        HideOthers,
        /// Minimizes the current window.
        Minimize,
        /// Resets the application database.
        ResetDatabase,
        /// Shows all hidden windows.
        ShowAll,
        /// Toggles fullscreen mode.
        ToggleFullScreen,
        /// Zooms the window.
        Zoom,
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());

    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());

    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(quit);

    // When ed logs to stdout rather than the log file, avoid registering
    // handlers for both `OpenLog` and `RevealLogInFileManager`, as the log file
    // does not exist in that scenario and these actions would error.
    if !crate::stdout_is_a_pty() {
        cx.on_action(|_: &OpenLog, cx| {
            with_active_or_new_workspace(cx, |workspace, window, cx| {
                open_log_file(workspace, window, cx);
            });
        })
        .on_action(|_: &workspace::RevealLogInFileManager, cx| {
            cx.reveal_path(paths::log_file().as_path());
        });
    }

    cx.on_action(|_: &About, cx| {
        open_about_window(cx);
    });
}

pub(crate) fn build_window_options(display_uuid: Option<Uuid>, cx: &mut App) -> WindowOptions {
    let display = display_uuid
        .and_then(|uuid| cx.displays().into_iter().find(|display| display.uuid().ok() == Some(uuid)))
        .or_else(|| frontmost_window_display(cx))
        .or_else(|| cx.main_display());

    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(TRAFFIC_LIGHT_POSITION.0), px(TRAFFIC_LIGHT_POSITION.1))),
        }),
        window_bounds: Some(workspace::new_window_bounds(display.as_ref().map(|display| display.id()), cx)),
        focus: false,
        show: false,
        kind: WindowKind::Normal,
        is_movable: true,
        // ed draws its own titlebar and moves the window via [`Window::start_window_move`],
        // so on macOS AppKit should not own titlebar dragging. This avoids the titlebar
        // click delay from AppKit's drag disambiguation (first observed on macOS 27) while
        // keeping the window movable and the Window-menu tiling items enabled. No-op on
        // other platforms.
        app_owns_titlebar_drag: true,
        display_id: display.map(|display| display.id()),
        window_background: cx.theme().window_background_appearance(),
        window_min_size: Some(gpui::Size { width: px(360.0), height: px(240.0) }),
        ..Default::default()
    }
}

/// The display of ed's frontmost window. Unlike the active window, this
/// doesn't need ed to be the active app.
fn frontmost_window_display(cx: &mut App) -> Option<Rc<dyn PlatformDisplay>> {
    let window = cx.window_stack()?.into_iter().next()?;
    window.update(cx, |_, window, cx| window.display(cx)).ok().flatten()
}

pub(crate) fn initialize_workspace(app_state: Arc<AppState>, cx: &mut App) {
    cx.set_cursor_hide_mode(gpui::CursorHideMode::OnTypingAndAction);
    init_app_appearance(cx);

    cx.observe_new(|_: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };

        // The window's workspace can be replaced, so look up the current root
        // when the window is asked to close.
        window.on_window_should_close(cx, move |window, cx| {
            let Some(workspace) = window.root::<Workspace>().flatten() else {
                return true;
            };
            // We'll handle closing asynchronously
            workspace.update(cx, |workspace, cx| workspace.close_window(&CloseWindow, window, cx));
            false
        });
    })
    .detach();

    cx.observe_new(move |workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };

        let workspace_handle = cx.entity();
        let center_pane = workspace.active_pane().clone();
        initialize_pane(workspace, &center_pane, window, cx);
        // Created with the workspace too, before the `PaneAdded` subscription below.
        let git_history_pane = workspace.git_history_pane().clone();
        initialize_pane(workspace, &git_history_pane, window, cx);

        cx.subscribe_in(&workspace_handle, window, {
            move |workspace, _, event, window, cx| match event {
                workspace::Event::PaneAdded(pane) => {
                    initialize_pane(workspace, pane, window, cx);
                }
                workspace::Event::OpenBundledFile { text, title, language } => {
                    open_bundled_file(workspace, text.clone(), title, language, window, cx);
                }
                _ => {}
            }
        })
        .detach();

        let search_button = cx.new(|_| SearchButton::new());
        let diagnostic_summary = cx.new(|cx| DiagnosticIndicator::new(workspace, cx));
        let activity_indicator = activity_indicator::ActivityIndicator::new(workspace, window, cx);
        let active_buffer_encoding = cx.new(|_| encoding_selector::ActiveBufferEncoding::new(workspace));
        let active_buffer_language = cx.new(|_| language_selector::ActiveBufferLanguage::new(workspace));
        let image_info = cx.new(|_cx| ImageInfo::new(workspace));

        let lsp_button_menu_handle = PopoverMenuHandle::default();
        let lsp_button = cx.new(|cx| LspButton::new(workspace, lsp_button_menu_handle.clone(), window, cx));
        workspace.register_action({
            move |_, _: &lsp_button::ToggleMenu, window, cx| {
                lsp_button_menu_handle.toggle(window, cx);
            }
        });

        let cursor_position = cx.new(|_| CursorPosition::new(workspace));
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_left_item(search_button, window, cx);
            status_bar.add_left_item(lsp_button, window, cx);
            status_bar.add_left_item(diagnostic_summary, window, cx);
            status_bar.add_left_item(activity_indicator, window, cx);
            status_bar.add_right_item(active_buffer_encoding, window, cx);
            status_bar.add_right_item(active_buffer_language, window, cx);
            status_bar.add_right_item(cursor_position, window, cx);
            status_bar.add_right_item(image_info, window, cx);
        });

        let panels_task = initialize_panels(window, cx);
        workspace.set_panels_task(panels_task);
        register_actions(Arc::clone(&app_state), workspace, window, cx);

        if !workspace.has_active_modal(window, cx) {
            workspace.focus_handle(cx).focus(window, cx);
        }
    })
    .detach();
}

fn initialize_panels(window: &Window, cx: &Context<Workspace>) -> Task<anyhow::Result<()>> {
    cx.spawn_in(window, async move |workspace_handle, cx| {
        fn add_panel(
            panel: anyhow::Result<Entity<impl workspace::Panel>>,
            workspace_handle: &WeakEntity<Workspace>,
            cx: &mut gpui::AsyncWindowContext,
        ) {
            if let Some(panel) = panel.context("failed to load panel").log_err() {
                workspace_handle
                    .update_in(cx, |workspace, window, cx| {
                        workspace.add_panel(panel, window, cx);
                    })
                    .log_err();
            }
        }

        let project_panel = ProjectPanel::load(&workspace_handle, cx);
        let terminal_panel = TerminalPanel::load(&workspace_handle, cx);
        let git_panel = GitPanel::load(&workspace_handle, cx);
        add_panel(project_panel, &workspace_handle, cx);
        add_panel(terminal_panel, &workspace_handle, cx);
        add_panel(git_panel, &workspace_handle, cx);

        workspace_handle.update(cx, |workspace, cx| {
            workspace.finish_dock_restoration(cx);
        })?;

        anyhow::Ok(())
    })
}

fn register_actions(app_state: Arc<AppState>, workspace: &mut Workspace, _: &mut Window, _cx: &mut Context<Workspace>) {
    workspace
        .register_action(|_, _: &Minimize, window, _| {
            window.minimize_window();
        })
        .register_action(|_, _: &Zoom, window, _| {
            window.zoom_window();
        })
        .register_action(|_, _: &ToggleFullScreen, window, _| {
            window.toggle_fullscreen();
        })
        .register_action(|workspace, action: &OpenBrowser, _window, cx| {
            // Parse and validate the URL to ensure it's properly formatted
            match url::Url::parse(&action.url) {
                Ok(parsed_url) => {
                    // Use the parsed URL's string representation which is properly escaped
                    cx.open_url(parsed_url.as_str());
                }
                Err(e) => {
                    workspace.show_error(
                        format!(
                            "Opening this URL in a browser failed because the URL is invalid: {}\n\nError was: {e}",
                            action.url
                        ),
                        cx,
                    );
                }
            }
        })
        .register_action(|workspace, action: &workspace::Open, window, cx| {
            workspace::prompt_for_open_path_and_open(
                workspace,
                PathPromptOptions { files: true, directories: true, multiple: true, prompt: None },
                action.create_new_window.unwrap_or(false),
                window,
                cx,
            );
        })
        .register_action(|workspace, _: &workspace::OpenFiles, window, cx| {
            workspace::prompt_for_open_path_and_open(
                workspace,
                PathPromptOptions { files: true, directories: true, multiple: true, prompt: None },
                true,
                window,
                cx,
            );
        })
        .register_action(|_, _: &ed_actions::IncreaseUiFontSize, _window, cx| {
            font::adjust_ui_font_size(cx, |size| size + px(1.0));
        })
        .register_action(|_, _: &ed_actions::DecreaseUiFontSize, _window, cx| {
            font::adjust_ui_font_size(cx, |size| size - px(1.0));
        })
        .register_action(|_, _: &ed_actions::ResetUiFontSize, _window, cx| {
            font::reset_ui_font_size(cx);
        })
        .register_action(|_, _: &ed_actions::IncreaseBufferFontSize, _window, cx| {
            font::adjust_buffer_font_size(cx, |size| size + px(1.0));
        })
        .register_action(|_, _: &ed_actions::DecreaseBufferFontSize, _window, cx| {
            font::adjust_buffer_font_size(cx, |size| size - px(1.0));
        })
        .register_action(|_, _: &ed_actions::ResetBufferFontSize, _window, cx| {
            font::reset_buffer_font_size(cx);
        })
        .register_action(|_, _: &ed_actions::ResetAllZoom, _window, cx| {
            font::reset_ui_font_size(cx);
            font::reset_buffer_font_size(cx);
        })
        .register_action(
            |workspace: &mut Workspace, _: &ToggleFocus, window: &mut Window, cx: &mut Context<Workspace>| {
                workspace.toggle_panel_focus::<ProjectPanel>(window, cx);
            },
        )
        .register_action(
            |workspace: &mut Workspace,
             _: &terminal_panel::ToggleFocus,
             window: &mut Window,
             cx: &mut Context<Workspace>| {
                workspace.toggle_panel_focus::<TerminalPanel>(window, cx);
            },
        )
        .register_action({
            let app_state = Arc::clone(&app_state);
            move |_, _: &NewWindow, _, cx| {
                open_new(workspace::OpenOptions::default(), Arc::clone(&app_state), cx, |_, _, cx| {
                    cx.activate(true);
                })
                .detach();
            }
        })
        .register_action({
            move |workspace, _: &CloseProject, window, cx| {
                let Some(window_handle) = window.window_handle().downcast::<Workspace>() else {
                    return;
                };
                let app_state = Arc::clone(workspace.app_state());
                cx.spawn_in(window, async move |workspace, cx| {
                    let should_continue = workspace
                        .update_in(cx, |workspace, window, cx| {
                            workspace.prepare_to_close(CloseIntent::ReplaceWindow, window, cx)
                        })?
                        .await?;
                    if should_continue {
                        cx.update(|_, cx| {
                            Workspace::new_local(
                                Vec::new(),
                                app_state,
                                Some(window_handle),
                                None,
                                workspace::OpenMode::Activate,
                                cx,
                            )
                        })?
                        .await?;
                    }
                    anyhow::Ok(())
                })
                .detach_and_log_err(cx);
            }
        })
        .register_action(move |_, _: &NewFile, _, cx| {
            open_new(workspace::OpenOptions::default(), Arc::clone(&app_state), cx, |workspace, window, cx| {
                Editor::new_file(workspace, &NewFile, window, cx);
            })
            .detach_and_log_err(cx);
        });
}

fn initialize_pane(workspace: &Workspace, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Workspace>) {
    pane.update(cx, |pane, cx| {
        pane.toolbar().update(cx, |toolbar, cx| {
            let solo_diff_style_toolbar = cx.new(SoloDiffStyleToolbar::new);
            toolbar.add_item(solo_diff_style_toolbar, window, cx);
            let file_diff_style_toolbar = cx.new(FileDiffStyleToolbar::new);
            toolbar.add_item(file_diff_style_toolbar, window, cx);
            let buffer_search_bar = cx.new(|cx| search::BufferSearchBar::new(window, cx));
            toolbar.add_item(buffer_search_bar, window, cx);
            let project_search_bar = cx.new(|_| ProjectSearchBar::new());
            toolbar.add_item(project_search_bar, window, cx);
            let lsp_log_item = cx.new(|_| LspLogToolbarItemView::new());
            toolbar.add_item(lsp_log_item, window, cx);
            let syntax_tree_item = cx.new(|_| language_tools::SyntaxTreeToolbarItemView::new());
            toolbar.add_item(syntax_tree_item, window, cx);
            let highlights_tree_item = cx.new(|_| language_tools::HighlightsTreeToolbarItemView::new());
            toolbar.add_item(highlights_tree_item, window, cx);
            let project_diff_toolbar = cx.new(|cx| ProjectDiffToolbar::new(workspace, cx));
            toolbar.add_item(project_diff_toolbar, window, cx);
            let staged_diff_toolbar = cx.new(|cx| StagedDiffToolbar::new(workspace, cx));
            toolbar.add_item(staged_diff_toolbar, window, cx);
            let unstaged_diff_toolbar = cx.new(|cx| UnstagedDiffToolbar::new(workspace, cx));
            toolbar.add_item(unstaged_diff_toolbar, window, cx);
            let branch_diff_toolbar = cx.new(BranchDiffToolbar::new);
            toolbar.add_item(branch_diff_toolbar, window, cx);
            let solo_diff_git_toolbar = cx.new(SoloDiffGitToolbar::new);
            toolbar.add_item(solo_diff_git_toolbar, window, cx);
            let commit_view_toolbar = cx.new(|_| CommitViewToolbar::new());
            toolbar.add_item(commit_view_toolbar, window, cx);
            let image_view_toolbar = cx.new(|_| image_viewer::ImageViewToolbarControls::new());
            toolbar.add_item(image_view_toolbar, window, cx);
        });
    });
}

fn open_about_window(cx: &mut App) {
    struct AboutWindow {
        ok_focus_handle: FocusHandle,
        app_icon: Arc<Image>,
        message: SharedString,
    }

    impl AboutWindow {
        fn new(cx: &mut Context<Self>) -> Self {
            let app_name = "ed";
            let version = env!("CARGO_PKG_VERSION");

            let message: SharedString = format!("{app_name} {version}").into();

            Self {
                ok_focus_handle: cx.focus_handle(),
                app_icon: Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    include_bytes!("../resources/app-icon.png").to_vec(),
                )),
                message,
            }
        }
    }

    impl Render for AboutWindow {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .id("about-window")
                .track_focus(&self.ok_focus_handle)
                .on_action(cx.listener(|_, _: &menu::Cancel, window, _cx| {
                    window.remove_window();
                }))
                .on_action(cx.listener(|_, _: &menu::Confirm, window, _cx| {
                    window.remove_window();
                }))
                .size_full()
                .bg(cx.theme().colors().editor_background)
                .text_color(cx.theme().colors().text)
                .p_4()
                .pt_8()
                .gap_3()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .gap_2()
                        .items_center()
                        .child(img(Arc::clone(&self.app_icon)).size_16().flex_none())
                        .child(Headline::new(self.message.clone())),
                )
                .child(Button::new("ok", "OK").width(px(96.)).style(ButtonStyle::Outlined).on_click(cx.listener(
                    |_, _, window, _cx| {
                        window.remove_window();
                    },
                )))
        }
    }

    impl Focusable for AboutWindow {
        fn focus_handle(&self, _cx: &App) -> FocusHandle {
            self.ok_focus_handle.clone()
        }
    }

    // Don't open about window twice
    if let Some(existing) = cx.windows().into_iter().find_map(|w| w.downcast::<AboutWindow>()) {
        existing
            .update(cx, |about_window, window, cx| {
                window.activate_window();
                about_window.ok_focus_handle.focus(window, cx);
            })
            .log_err();
        return;
    }

    let window_size = Size { width: px(280.), height: px(200.) };
    // Open on the display the user is working on rather than the primary one.
    let display_id = cx.main_display().map(|display| display.id());

    cx.open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("About ed".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.), px(12.))),
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(display_id, window_size, cx))),
            display_id,
            is_resizable: false,
            is_minimizable: false,
            kind: WindowKind::Floating,
            ..Default::default()
        },
        |window, cx| {
            let about_window = cx.new(AboutWindow::new);
            let focus_handle = about_window.read(cx).ok_focus_handle.clone();
            window.activate_window();
            focus_handle.focus(window, cx);
            about_window
        },
    )
    .log_err();
}

fn quit(_: &Quit, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut workspace_windows: Vec<WindowHandle<Workspace>> = cx.update(|cx| {
            cx.windows().into_iter().filter_map(|window| window.downcast::<Workspace>()).collect::<Vec<_>>()
        });

        // If multiple windows have unsaved changes, and need a save prompt,
        // prompt in the active window before switching to a different window.
        cx.update(|cx| {
            workspace_windows.sort_by_key(|window| window.is_active(cx) == Some(false));
        });

        if !workspace::prepare_windows_to_quit(&workspace_windows, cx).await {
            return Ok(());
        }

        cx.update(|cx| cx.quit());
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

fn open_log_file(workspace: &Workspace, window: &Window, cx: &Context<Workspace>) {
    const MAX_LINES: usize = 1000;
    let app_state = workspace.app_state();
    let languages = Arc::clone(&app_state.languages);
    let fs = Arc::clone(&app_state.fs);
    cx.spawn_in(window, async move |workspace, cx| {
        let log = {
            let result = futures::join!(
                fs.load(paths::old_log_file()),
                fs.load(paths::log_file()),
                languages.language_for_name("log")
            );
            match result {
                (Err(_), Err(e), _) => Err(e),
                (old_log, new_log, lang) => {
                    let mut lines = VecDeque::with_capacity(MAX_LINES);
                    for line in
                        old_log.iter().flat_map(|log| log.lines()).chain(new_log.iter().flat_map(|log| log.lines()))
                    {
                        if lines.len() == MAX_LINES {
                            lines.pop_front();
                        }
                        lines.push_back(line);
                    }
                    Ok((lines.into_iter().flat_map(|line| [line, "\n"]).collect::<String>(), lang.ok()))
                }
            }
        };

        let (log, log_language) = match log {
            Ok((log, log_language)) => (log, log_language),
            Err(e) => {
                struct OpenLogError;

                workspace
                    .update(cx, |workspace, cx| {
                        workspace.show_notification(NotificationId::unique::<OpenLogError>(), cx, |cx| {
                            cx.new(|cx| {
                                MessageNotification::new(
                                    format!(
                                        "Unable to access/open log file at path \
                                                    {}: {e:#}",
                                        paths::log_file().display()
                                    ),
                                    cx,
                                )
                            })
                        });
                    })
                    .ok();
                return;
            }
        };
        async move {
            let project = workspace.read_with(cx, |workspace, _| workspace.project().clone()).ok()?;
            let buffer = project.update(cx, |project, cx| project.create_buffer(log_language, false, cx)).await.ok()?;
            buffer.update(cx, |buffer, cx| {
                buffer.set_capability(Capability::ReadOnly, cx);
                buffer.set_text(log, cx);
            });

            let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx).with_title("Log".into()));

            let editor = cx
                .new_window_entity(|window, cx| {
                    let mut editor = Editor::for_multibuffer(buffer, Some(project), window, cx);
                    editor.set_read_only(true);
                    let last_multi_buffer_offset = editor.buffer().read(cx).len(cx);
                    editor.change_selections(editor::SelectionEffects::default(), window, cx, |s| {
                        s.select_ranges(Some(last_multi_buffer_offset..last_multi_buffer_offset));
                    });
                    editor
                })
                .ok()?;

            workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.add_item_to_active_pane(Box::new(editor), None, true, window, cx);
                })
                .ok()
        }
        .await;
    })
    .detach();
}

fn init_app_appearance(cx: &App) {
    // Force the native window chrome (border + titlebar) to match the constant theme, so the
    // dark theme doesn't render a light window border when the system is in light mode.
    let appearance = match cx.theme().appearance() {
        theme::Appearance::Light => gpui::WindowAppearance::Light,
        theme::Appearance::Dark => gpui::WindowAppearance::Dark,
    };
    cx.set_window_appearance(Some(appearance));
}

/// The keymap and menus are built-in and static: they're built once at startup, against the
/// keyboard layout active at launch.
pub(crate) fn initialize_keymap(cx: &mut App) {
    let bindings = keymap::keymap(cx);
    cx.bind_keys(bindings);
    let menus = app_menus(cx);
    cx.set_menus(menus);
    cx.set_dock_menu(vec![gpui::MenuItem::action("New Window", workspace::NewWindow)]);
}

fn open_bundled_file(
    workspace: &mut Workspace,
    text: Cow<'static, str>,
    title: &'static str,
    language: &'static str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let existing = workspace.items_of_type::<Editor>(cx).find(|editor| {
        editor.read_with(cx, |editor, cx| {
            editor.read_only(cx)
                && editor.title(cx).as_ref() == title
                && editor.buffer().read(cx).as_singleton().is_some_and(|buffer| buffer.read(cx).file().is_none())
        })
    });
    if let Some(existing) = existing {
        workspace.activate_item(&existing, true, true, window, cx);
        return;
    }

    let language = workspace.app_state().languages.language_for_name(language);
    cx.spawn_in(window, async move |workspace, cx| {
        let language = language.await.log_err();
        workspace
            .update_in(cx, move |workspace, window, cx| {
                let project = workspace.project().clone();
                let buffer = project.update(cx, move |project, cx| project.create_buffer(language, false, cx));
                cx.spawn_in(window, async move |workspace, cx| {
                    let buffer = buffer.await?;
                    buffer.update(cx, |buffer, cx| {
                        buffer.set_text(text.into_owned(), cx);
                        buffer.set_capability(Capability::ReadOnly, cx);
                    });
                    let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx).with_title(title.into()));
                    workspace.update_in(cx, |workspace, window, cx| {
                        workspace.add_item_to_active_pane(
                            Box::new(cx.new(|cx| {
                                let mut editor = Editor::for_multibuffer(buffer, Some(project.clone()), window, cx);
                                editor.set_read_only(true);
                                editor.set_should_serialize(false);
                                editor
                            })),
                            None,
                            true,
                            window,
                            cx,
                        );
                    })
                })
            })?
            .await
    })
    .detach_and_log_err(cx);
}
