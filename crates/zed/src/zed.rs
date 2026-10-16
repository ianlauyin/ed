mod app_menus;

pub(crate) mod mac_only_instance;

pub(crate) mod move_to_applications;
mod open_listener;
mod open_url_modal;
mod quick_action_bar;

use anyhow::Context as _;
pub use app_menus::*;
use assets::Assets;

use breadcrumbs::Breadcrumbs;
use collections::VecDeque;
use editor::{Editor, MultiBuffer};
use git_ui::branch_diff::BranchDiffToolbar;
use git_ui::commit_view::CommitViewToolbar;
use git_ui::git_panel::GitPanel;
use git_ui::project_diff::ProjectDiffToolbar;
use git_ui::solo_diff_view::{SoloDiffGitToolbar, SoloDiffStyleToolbar};
use git_ui::staged_diff::StagedDiffToolbar;
use git_ui::unstaged_diff::UnstagedDiffToolbar;
use git_ui_core::file_diff_view::FileDiffStyleToolbar;
use gpui::{
    Action, App, AppContext as _, ClipboardItem, Context, DismissEvent, Entity, FocusHandle,
    Focusable, Image, ImageFormat, ParentElement, PathPromptOptions, PromptLevel, ReadGlobal,
    SharedString, Size, Task, TaskExt, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, actions, img, point, px,
};
use image_viewer::ImageInfo;
use language::Capability;
use language_onboarding::BasedPyrightBanner;
use language_tools::lsp_button::{self, LspButton};
use language_tools::lsp_log_view::LspLogToolbarItemView;
pub use open_listener::*;
use outline_panel::OutlinePanel;
use paths::local_tasks_file_relative_path;
use project::{
    DirectoryLister,
    project_settings::{SettingsObserver, SettingsObserverEvent},
};
use project_panel::ProjectPanel;
use quick_action_bar::QuickActionBar;
use recent_projects::open_remote_project;
use rope::Rope;
use search::project_search::ProjectSearchBar;
use settings::{
    DefaultOpenBehavior, Settings, SettingsStore, initial_tasks_content, update_settings_file,
};
use settings_content::app_metadata::{AppCommitSha, AppVersion};
#[cfg(debug_assertions)]
use workspace::workspace_error::{ErrorAction, ErrorSeverity, WorkspaceError};

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
    sync::atomic::{self, AtomicBool},
};
use terminal_view::terminal_panel::{self, TerminalPanel};
use theme::ActiveTheme;
use theme_settings::ThemeSettings;
use ui::{Navigable, NavigableEntry, PopoverMenuHandle, TintColor, prelude::*};
use util::rel_path::RelPath;
use util::{ResultExt, asset_str, maybe};
use uuid::Uuid;
use workspace::Pane;
use workspace::notifications::{NotificationId, dismiss_app_notification, show_app_notification};

use workspace::{
    AppState, MultiWorkspace, NewFile, NewWindow, OpenLog, Workspace, WorkspaceSettings,
    create_and_open_local_file, notifications::simple_message_notification::MessageNotification,
    open_new,
};
use workspace::{CloseProject, CloseWindow, with_active_or_new_workspace};
use zed_actions::{
    About, GetMerch, OpenBrowser, OpenDocs, OpenProjectTasks, OpenStatusPage, OpenZedUrl, Quit,
};

const DOCS_URL: &str = "https://zed.dev/docs/";
const STATUS_URL: &str = "https://status.zed.dev";
const MERCH_URL: &str = "https://merch.zed.dev/";

pub struct CrashHandler(pub Arc<crashes::Client>);

impl gpui::Global for CrashHandler {}

actions!(
    zed,
    [
        /// Hides the application window.
        Hide,
        /// Hides all other application windows.
        HideOthers,
        /// Minimizes the current window.
        Minimize,
        /// Opens the tasks panel.
        OpenTasks,
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

actions!(
    dev,
    [
        /// Opens a prompt to enter a URL to open.
        OpenUrlPrompt,
        /// Dumps the current accessibility tree (the last update sent to the
        /// platform adapter) to a new buffer as JSON, for debugging what is
        /// exposed to assistive technology.
        DumpAccessibilityTree,
        /// Copies the current accessibility tree to the clipboard as JSON,
        /// without opening a buffer. See [`DumpAccessibilityTree`].
        CopyAccessibilityTree,
    ]
);

/// Serializes the window's most recent accessibility tree to JSON for the
/// `dev: dump/copy accessibility tree` actions, falling back to a friendly
/// placeholder when no tree has been built yet.
fn accessibility_tree_dump(window: &Window) -> String {
    window.debug_a11y_tree_json().unwrap_or_else(|| {
        "No accessibility tree has been built yet. The tree is only \
         produced once assistive technology (e.g. a screen reader) is \
         active for this window."
            .to_string()
    })
}

#[cfg(debug_assertions)]
actions!(
    dev,
    [
        /// Show an error on the workspace level.
        ShowWorkspaceError
    ]
);

pub fn init(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());

    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());

    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(quit);

    // When Zed logs to stdout rather than the log file, avoid registering
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

    cx.on_action(|_: &zed_actions::OpenLicenses, cx| {
        with_active_or_new_workspace(cx, |workspace, window, cx| {
            open_bundled_file(
                workspace,
                asset_str::<Assets>("licenses.md"),
                "Open Source License Attribution",
                "Markdown",
                window,
                cx,
            );
        });
    })
    .on_action(|_: &OpenTasks, cx| {
        with_active_or_new_workspace(cx, |_, window, cx| {
            open_settings_file(
                paths::tasks_file(),
                || settings::initial_tasks_content().as_ref().into(),
                window,
                cx,
            );
        });
    })
    .on_action(|_: &About, cx| {
        open_about_window(cx);
    });
}

fn bind_on_window_closed(cx: &mut App) -> Option<gpui::Subscription> {
    {
        WorkspaceSettings::get_global(cx)
            .on_last_window_closed
            .is_quit_app()
            .then(|| {
                cx.on_window_closed(|cx, _window_id| {
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                })
            })
    }
}

pub fn build_window_options(display_uuid: Option<Uuid>, cx: &mut App) -> WindowOptions {
    let display = display_uuid.and_then(|uuid| {
        cx.displays()
            .into_iter()
            .find(|display| display.uuid().ok() == Some(uuid))
    });
    let app_id = settings_content::app_metadata::APP_ID;
    let window_decorations = match std::env::var("ZED_WINDOW_DECORATIONS") {
        Ok(val) if val == "server" => gpui::WindowDecorations::Server,
        Ok(val) if val == "client" => gpui::WindowDecorations::Client,
        _ => match WorkspaceSettings::get_global(cx).window_decorations {
            settings::WindowDecorations::Server => gpui::WindowDecorations::Server,
            settings::WindowDecorations::Client => gpui::WindowDecorations::Client,
        },
    };

    let use_system_window_tabs = WorkspaceSettings::get_global(cx).use_system_window_tabs;

    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.0), px(9.0))),
        }),
        window_bounds: None,
        focus: false,
        show: false,
        kind: WindowKind::Normal,
        is_movable: true,
        // Zed draws its own titlebar and moves the window via [`Window::start_window_move`],
        // so on macOS AppKit should not own titlebar dragging. This avoids the titlebar
        // click delay from AppKit's drag disambiguation (first observed on macOS 27) while
        // keeping the window movable and the Window-menu tiling items enabled. No-op on
        // other platforms.
        app_owns_titlebar_drag: true,
        display_id: display.map(|display| display.id()),
        window_background: cx.theme().window_background_appearance(),
        app_id: Some(app_id.to_owned()),
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        icon: APP_ICON.as_ref().cloned(),
        window_decorations: Some(window_decorations),
        window_min_size: Some(gpui::Size {
            width: px(360.0),
            height: px(240.0),
        }),
        tabbing_identifier: if use_system_window_tabs {
            Some(String::from("zed"))
        } else {
            None
        },
        ..Default::default()
    }
}

pub fn initialize_workspace(app_state: Arc<AppState>, cx: &mut App) {
    let mut _on_close_subscription = bind_on_window_closed(cx);
    cx.observe_global::<SettingsStore>(move |cx| {
        // A 1.92 regression causes unused-assignment to trigger on this variable.
        _ = _on_close_subscription.is_some();
        _on_close_subscription = bind_on_window_closed(cx);
    })
    .detach();

    init_cursor_hide_mode(cx);
    init_app_appearance(cx);
    init_reduce_motion(cx);
    init_global_config_error_notifications(cx);

    cx.observe_new(|_: &mut MultiWorkspace, window, cx| {
        let Some(window) = window else {
            return;
        };

        let multi_workspace_handle = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            multi_workspace_handle
                .update(cx, |multi_workspace, cx| {
                    // We'll handle closing asynchronously
                    multi_workspace.close_window(&CloseWindow, window, cx);
                    false
                })
                .unwrap_or(true)
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

        cx.subscribe_in(&workspace_handle, window, {
            move |workspace, _, event, window, cx| match event {
                workspace::Event::PaneAdded(pane) => {
                    initialize_pane(workspace, pane, window, cx);
                }
                workspace::Event::OpenBundledFile {
                    text,
                    title,
                    language,
                } => open_bundled_file(workspace, text.clone(), title, language, window, cx),
                _ => {}
            }
        })
        .detach();

        if let Some(specs) = window.gpu_specs() {
            log::info!("Using GPU: {:?}", specs);
            if let Some(crash_client) = cx.try_global::<CrashHandler>() {
                crashes::set_gpu_info(&crash_client.0, specs);
            }
        }

        let search_button = cx.new(|_| search::search_status_button::SearchButton::new());
        let diagnostic_summary =
            cx.new(|cx| diagnostics::items::DiagnosticIndicator::new(workspace, cx));
        let active_file_name = cx.new(|_| workspace::active_file_name::ActiveFileName::new());
        let activity_indicator = activity_indicator::ActivityIndicator::new(workspace, window, cx);
        let active_buffer_encoding =
            cx.new(|_| encoding_selector::ActiveBufferEncoding::new(workspace));
        let active_buffer_language =
            cx.new(|_| language_selector::ActiveBufferLanguage::new(workspace));
        let active_toolchain_language =
            cx.new(|cx| toolchain_selector::ActiveToolchain::new(workspace, window, cx));
        let pending_keystrokes_indicator =
            cx.new(|cx| which_key::PendingKeystrokesIndicator::new(window, cx));
        let image_info = cx.new(|_cx| ImageInfo::new(workspace));

        let lsp_button_menu_handle = PopoverMenuHandle::default();
        let lsp_button =
            cx.new(|cx| LspButton::new(workspace, lsp_button_menu_handle.clone(), window, cx));
        workspace.register_action({
            move |_, _: &lsp_button::ToggleMenu, window, cx| {
                lsp_button_menu_handle.toggle(window, cx);
            }
        });

        let cursor_position =
            cx.new(|_| go_to_line::cursor_position::CursorPosition::new(workspace));
        let line_ending_indicator =
            cx.new(|_| line_ending_selector::LineEndingIndicator::default());
        let git_blame_status = cx.new(|_| git_ui::GitBlameStatus::default());
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_left_item(search_button, window, cx);
            status_bar.add_left_item(lsp_button, window, cx);
            status_bar.add_left_item(diagnostic_summary, window, cx);
            status_bar.add_left_item(active_file_name, window, cx);
            status_bar.add_left_item(git_blame_status, window, cx);
            status_bar.add_left_item(activity_indicator, window, cx);
            status_bar.add_right_item(active_buffer_encoding, window, cx);
            status_bar.add_right_item(active_buffer_language, window, cx);
            status_bar.add_right_item(active_toolchain_language, window, cx);
            status_bar.add_right_item(line_ending_indicator, window, cx);
            status_bar.add_right_item(cursor_position, window, cx);
            status_bar.add_right_item(image_info, window, cx);
            // Keep this last so it stays leftmost and can change without moving the other items.
            status_bar.add_right_item(pending_keystrokes_indicator, window, cx);
        });

        let panels_task = initialize_panels(window, cx);
        workspace.set_panels_task(panels_task);
        register_actions(app_state.clone(), workspace, window, cx);

        if !workspace.has_active_modal(window, cx) {
            workspace.focus_handle(cx).focus(window, cx);
        }
    })
    .detach();
}

fn initialize_panels(window: &mut Window, cx: &mut Context<Workspace>) -> Task<anyhow::Result<()>> {
    cx.spawn_in(window, async move |workspace_handle, cx| {
        let project_panel = ProjectPanel::load(workspace_handle.clone(), cx.clone());
        let outline_panel = OutlinePanel::load(workspace_handle.clone(), cx.clone());
        let terminal_panel = TerminalPanel::load(workspace_handle.clone(), cx.clone());
        let git_panel = GitPanel::load(workspace_handle.clone(), cx.clone());

        async fn add_panel_when_ready(
            panel_task: impl Future<Output = anyhow::Result<Entity<impl workspace::Panel>>> + 'static,
            workspace_handle: WeakEntity<Workspace>,
            mut cx: gpui::AsyncWindowContext,
        ) {
            if let Some(panel) = panel_task.await.context("failed to load panel").log_err()
            {
                workspace_handle
                    .update_in(&mut cx, |workspace, window, cx| {
                        workspace.add_panel(panel, window, cx);
                    })
                    .log_err();
            }
        }

        futures::join!(
            add_panel_when_ready(project_panel, workspace_handle.clone(), cx.clone()),
            add_panel_when_ready(outline_panel, workspace_handle.clone(), cx.clone()),
            add_panel_when_ready(terminal_panel, workspace_handle.clone(), cx.clone()),
            add_panel_when_ready(git_panel, workspace_handle.clone(), cx.clone()),
        );

        workspace_handle.update(cx, |workspace, cx| {
            workspace.finish_dock_restoration(cx);
        })?;

        anyhow::Ok(())
    })
}

fn register_actions(
    app_state: Arc<AppState>,
    workspace: &mut Workspace,
    _: &mut Window,
    _cx: &mut Context<Workspace>,
) {
    workspace
        .register_action(|_, _: &OpenDocs, _, cx| cx.open_url(DOCS_URL))
        .register_action(|_, _: &OpenStatusPage, _, cx| cx.open_url(STATUS_URL))
        .register_action(|_, _: &GetMerch, _, cx| cx.open_url(MERCH_URL))
        .register_action(
            |workspace: &mut Workspace,
             _: &DumpAccessibilityTree,
             window: &mut Window,
             cx: &mut Context<Workspace>| {
                let json = accessibility_tree_dump(window);
                let language = workspace.app_state().languages.language_for_name("JSON");
                let project = workspace.project().clone();
                cx.spawn_in(window, async move |workspace, cx| {
                    let language = language.await.log_err();
                    let buffer = project
                        .update(cx, |project, cx| {
                            project.create_buffer(language, true, cx)
                        })
                        .await?;
                    buffer.update(cx, |buffer, cx| {
                        buffer.set_text(json, cx);
                    });
                    workspace.update_in(cx, |workspace, window, cx| {
                        let title = "Accessibility Tree".to_string();
                        let buffer = cx.new(|cx| {
                            MultiBuffer::singleton(buffer, cx).with_title(title.clone())
                        });
                        let editor = cx.new(|cx| {
                            let mut editor = Editor::for_multibuffer(
                                buffer,
                                Some(project),
                                window,
                                cx,
                            );
                            editor.set_breadcrumb_header(title);
                            editor
                        });
                        workspace.add_item_to_active_pane(
                            Box::new(editor),
                            None,
                            true,
                            window,
                            cx,
                        );
                    })
                })
                .detach_and_log_err(cx);
            },
        )
        .register_action(
            |_workspace: &mut Workspace,
             _: &CopyAccessibilityTree,
             window: &mut Window,
             cx: &mut Context<Workspace>| {
                let json = accessibility_tree_dump(window);
                cx.write_to_clipboard(ClipboardItem::new_string(json));
            },
        )
        .register_action(|_, _: &Minimize, window, _| {
            window.minimize_window();
        })
        .register_action(|_, _: &Zoom, window, _| {
            window.zoom_window();
        })
        .register_action(|_, _: &ToggleFullScreen, window, cx| {
            let use_simple_fullscreen = PlatformStyle::platform() == PlatformStyle::Mac
                && (window.is_simple_fullscreen()
                    || (!window.is_fullscreen()
                        && WorkspaceSettings::get_global(cx).fullscreen_mode
                            == settings::FullscreenMode::Simple));
            if use_simple_fullscreen {
                window.toggle_simple_fullscreen();
            } else {
                window.toggle_fullscreen();
            }
        })
        .register_action(|_, action: &OpenZedUrl, _, cx| {
            OpenListener::global(cx).open(RawOpenRequest {
                urls: vec![String::from(&*action.url)],
                ..Default::default()
            })
        })
        .register_action(|workspace, _: &OpenUrlPrompt, window, cx| {
            workspace.toggle_modal(window, cx, |window, cx| {
                open_url_modal::OpenUrlModal::new(window, cx)
            });
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
                workspace.app_state().clone(),
                PathPromptOptions {
                    files: true,
                    directories: true,
                    multiple: true,
                    prompt: None,
                },
                action.create_new_window.unwrap_or_else(|| {
                    matches!(
                        WorkspaceSettings::get_global(cx).default_open_behavior,
                        DefaultOpenBehavior::NewWindow
                    )
                }),
                window,
                cx,
            );
        })
        .register_action(|workspace, _: &workspace::OpenFiles, window, cx| {
            let directories = cx.can_select_mixed_files_and_dirs();
            workspace::prompt_for_open_path_and_open(
                workspace,
                workspace.app_state().clone(),
                PathPromptOptions {
                    files: true,
                    directories,
                    multiple: true,
                    prompt: None,
                },
                true,
                window,
                cx,
            );
        })
        .register_action(|workspace, action: &zed_actions::OpenRemote, window, cx| {
            if !action.from_existing_connection {
                cx.propagate();
                return;
            }
            // You need existing remote connection to open it this way
            if workspace.project().read(cx).is_local() {
                return;
            }
            let create_new_window = action.create_new_window.unwrap_or_else(|| {
                matches!(
                    WorkspaceSettings::get_global(cx).default_open_behavior,
                    DefaultOpenBehavior::NewWindow
                )
            });

            let paths = workspace.prompt_for_open_path(
                PathPromptOptions {
                    files: true,
                    directories: true,
                    multiple: true,
                    prompt: None,
                },
                DirectoryLister::Project(workspace.project().clone()),
                window,
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let Some(paths) = paths.await.log_err().flatten() else {
                    return;
                };
                if let Some(task) = this
                    .update_in(cx, |this, window, cx| {
                        open_new_ssh_project_from_project(
                            this,
                            paths,
                            create_new_window,
                            window,
                            cx,
                        )
                    })
                    .log_err()
                {
                    task.await.log_err();
                }
            })
            .detach()
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zed_actions::IncreaseUiFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let ui_font_size = ThemeSettings::get_global(cx).ui_font_size(cx) + px(1.0);
                        let _ = settings
                            .theme
                            .ui_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(ui_font_size)).into());
                    });
                } else {
                    theme_settings::adjust_ui_font_size(cx, |size| size + px(1.0));
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zed_actions::DecreaseUiFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let ui_font_size = ThemeSettings::get_global(cx).ui_font_size(cx) - px(1.0);
                        let _ = settings
                            .theme
                            .ui_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(ui_font_size)).into());
                    });
                } else {
                    theme_settings::adjust_ui_font_size(cx, |size| size - px(1.0));
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zed_actions::ResetUiFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, _| {
                        settings.theme.ui_font_size = None;
                    });
                } else {
                    theme_settings::reset_ui_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zed_actions::IncreaseBufferFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let buffer_font_size =
                            ThemeSettings::get_global(cx).buffer_font_size(cx) + px(1.0);
                        let _ = settings
                            .theme
                            .buffer_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(buffer_font_size)).into());
                    });
                } else {
                    theme_settings::increase_buffer_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zed_actions::DecreaseBufferFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, cx| {
                        let buffer_font_size =
                            ThemeSettings::get_global(cx).buffer_font_size(cx) - px(1.0);
                        let _ = settings
                            .theme
                            .buffer_font_size
                            .insert(f32::from(theme_settings::clamp_font_size(buffer_font_size)).into());
                    });
                } else {
                    theme_settings::decrease_buffer_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zed_actions::ResetBufferFontSize, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, _| {
                        settings.theme.buffer_font_size = None;
                    });
                } else {
                    theme_settings::reset_buffer_font_size(cx);
                }
            }
        })
        .register_action({
            let fs = app_state.fs.clone();
            move |_, action: &zed_actions::ResetAllZoom, _window, cx| {
                if action.persist {
                    update_settings_file(fs.clone(), cx, move |settings, _| {
                        settings.theme.ui_font_size = None;
                        settings.theme.buffer_font_size = None;
                        settings.theme.agent_ui_font_size = None;
                        settings.theme.agent_buffer_font_size = None;
                    });
                } else {
                    theme_settings::reset_ui_font_size(cx);
                    theme_settings::reset_buffer_font_size(cx);
                    theme_settings::reset_agent_ui_font_size(cx);
                    theme_settings::reset_agent_buffer_font_size(cx);
                }
            }
        })
        .register_action(open_project_tasks_file)
        .register_action(open_worktree_setup_tasks_file)
        .register_action(
            |workspace: &mut Workspace,
             _: &zed_actions::project_panel::ToggleFocus,
             window: &mut Window,
             cx: &mut Context<Workspace>| {
                workspace.toggle_panel_focus::<ProjectPanel>(window, cx);
            },
        )
        .register_action(
            |workspace: &mut Workspace,
             _: &outline_panel::ToggleFocus,
             window: &mut Window,
             cx: &mut Context<Workspace>| {
                workspace.toggle_panel_focus::<OutlinePanel>(window, cx);
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
            let app_state = app_state.clone();
            move |_, _: &NewWindow, _, cx| {
                open_new(
                    Default::default(),
                    app_state.clone(),
                    cx,
                    |workspace, window, cx| {
                        cx.activate(true);
                        initialize_new_window(workspace, window, cx);
                    },
                )
                .detach();
            }
        })
        .register_action({
            move |workspace, _: &CloseProject, window, cx| {
                let Some(window_handle) = window.window_handle().downcast::<MultiWorkspace>() else {
                    return;
                };
                let old_group_key = workspace.project_group_key(cx);
                cx.spawn_in(window, async move |_, cx| {
                    let task = window_handle.update(cx, |multi_workspace, window, cx| {
                        multi_workspace.remove_project_group(&old_group_key, window, cx)
                    })?;
                    task.await?;
                    anyhow::Ok(())
                })
                .detach_and_log_err(cx);
            }
        })
        .register_action({
            let app_state = app_state.clone();
            move |_, _: &NewFile, _, cx| {
                open_new(
                    Default::default(),
                    app_state.clone(),
                    cx,
                    |workspace, window, cx| {
                        Editor::new_file(workspace, &Default::default(), window, cx)
                    },
                )
                .detach_and_log_err(cx);
            }
        });

    #[cfg(debug_assertions)]
    workspace.register_action(|workspace, _: &ShowWorkspaceError, _, cx| {
        struct DebugError;
        struct SecondDebugError;

        impl WorkspaceError for DebugError {
            fn primary_message(&self) -> SharedString {
                SharedString::new_static(
                    "Error: Prepare rename via rust-analyzer failed: No references found at position",
                )
            }

            fn severity(&self) -> ErrorSeverity {
                ErrorSeverity::Warning
            }

            fn primary_action(&self) -> ErrorAction {
                ErrorAction::dismiss()
            }
        }

        impl WorkspaceError for SecondDebugError {
            fn primary_message(&self) -> SharedString {
                SharedString::new_static("This is some error to ignore.")
            }

            fn severity(&self) -> ErrorSeverity {
                ErrorSeverity::Error
            }

            fn primary_action(&self) -> ErrorAction {
                ErrorAction::dismiss()
            }
        }

        workspace.show_error(DebugError, cx);
        workspace.show_error(SecondDebugError, cx);
    });
}

fn initialize_pane(
    workspace: &Workspace,
    pane: &Entity<Pane>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    pane.update(cx, |pane, cx| {
        pane.toolbar().update(cx, |toolbar, cx| {
            let solo_diff_style_toolbar = cx.new(SoloDiffStyleToolbar::new);
            toolbar.add_item(solo_diff_style_toolbar, window, cx);
            let file_diff_style_toolbar = cx.new(FileDiffStyleToolbar::new);
            toolbar.add_item(file_diff_style_toolbar, window, cx);
            let breadcrumbs = cx.new(|_| Breadcrumbs::new());
            toolbar.add_item(breadcrumbs, window, cx);
            let buffer_search_bar = cx.new(|cx| {
                search::BufferSearchBar::new(
                    Some(workspace.project().read(cx).languages().clone()),
                    window,
                    cx,
                )
            });
            toolbar.add_item(buffer_search_bar.clone(), window, cx);
            let quick_action_bar =
                cx.new(|cx| QuickActionBar::new(buffer_search_bar, workspace, cx));
            toolbar.add_item(quick_action_bar, window, cx);
            let diagnostic_editor_controls = cx.new(|_| diagnostics::ToolbarControls::new());
            toolbar.add_item(diagnostic_editor_controls, window, cx);
            let project_search_bar = cx.new(|_| ProjectSearchBar::new());
            toolbar.add_item(project_search_bar, window, cx);
            let lsp_log_item = cx.new(|_| LspLogToolbarItemView::new());
            toolbar.add_item(lsp_log_item, window, cx);
            let syntax_tree_item = cx.new(|_| language_tools::SyntaxTreeToolbarItemView::new());
            toolbar.add_item(syntax_tree_item, window, cx);
            let highlights_tree_item =
                cx.new(|_| language_tools::HighlightsTreeToolbarItemView::new());
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
            let basedpyright_banner = cx.new(|cx| BasedPyrightBanner::new(workspace, cx));
            toolbar.add_item(basedpyright_banner, window, cx);
            let image_view_toolbar = cx.new(|_| image_viewer::ImageViewToolbarControls::new());
            toolbar.add_item(image_view_toolbar, window, cx);
        })
    });
}

fn open_about_window(cx: &mut App) {
    struct AboutWindow {
        focus_handle: FocusHandle,
        ok_entry: NavigableEntry,
        copy_entry: NavigableEntry,
        app_icon: Arc<Image>,
        message: SharedString,
        commit: Option<SharedString>,
        full_version: SharedString,
    }

    impl AboutWindow {
        fn new(cx: &mut Context<Self>) -> Self {
            let app_name = "Zed";
            let full_version: SharedString = AppVersion::global(cx).to_string().into();
            let version = env!("CARGO_PKG_VERSION");

            let debug = if cfg!(debug_assertions) {
                "(debug)"
            } else {
                ""
            };
            let message: SharedString = format!("{app_name} {version} {debug}").into();
            let commit = AppCommitSha::try_global(cx)
                .map(|sha| sha.full())
                .filter(|commit| !commit.is_empty())
                .map(SharedString::from);

            Self {
                focus_handle: cx.focus_handle(),
                ok_entry: NavigableEntry::focusable(cx),
                copy_entry: NavigableEntry::focusable(cx),
                app_icon: Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    include_bytes!("../resources/app-icon.png").to_vec(),
                )),
                message,
                commit,
                full_version,
            }
        }

        fn copy_details(&self, window: &mut Window, cx: &mut Context<Self>) {
            let content = match self.commit.as_ref() {
                Some(commit) => {
                    format!(
                        "{}\nCommit: {}\nVersion: {}",
                        self.message, commit, self.full_version
                    )
                }
                None => format!("{}\nVersion: {}", self.message, self.full_version),
            };
            cx.write_to_clipboard(ClipboardItem::new_string(content));
            window.remove_window();
        }
    }

    impl Render for AboutWindow {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let ok_is_focused = self.ok_entry.focus_handle.contains_focused(window, cx);
            let copy_is_focused = self.copy_entry.focus_handle.contains_focused(window, cx);

            Navigable::new(
                v_flex()
                    .id("about-window")
                    .track_focus(&self.focus_handle)
                    .on_action(cx.listener(|_, _: &menu::Cancel, window, _cx| {
                        window.remove_window();
                    }))
                    .min_w_0()
                    .size_full()
                    .bg(cx.theme().colors().editor_background)
                    .text_color(cx.theme().colors().text)
                    .p_4()
                    .when(true, |this| this.pt_10())
                    .gap_4()
                    .text_center()
                    .justify_between()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(img(self.app_icon.clone()).size_16().flex_none())
                            .child(Headline::new(self.message.clone()))
                            .when_some(self.commit.clone(), |this, commit| {
                                this.child(
                                    Label::new("Commit")
                                        .color(Color::Muted)
                                        .size(LabelSize::XSmall),
                                )
                                .child(Label::new(commit).size(LabelSize::Small))
                            })
                            .child(
                                Label::new("Version")
                                    .color(Color::Muted)
                                    .size(LabelSize::XSmall),
                            )
                            .child(Label::new(self.full_version.clone()).size(LabelSize::Small)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .track_focus(&self.ok_entry.focus_handle)
                                    .on_action(cx.listener(|_, _: &menu::Confirm, window, _cx| {
                                        window.remove_window();
                                    }))
                                    .child(
                                        Button::new("ok", "OK")
                                            .full_width()
                                            .style(ButtonStyle::OutlinedGhost)
                                            .toggle_state(ok_is_focused)
                                            .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                                            .on_click(cx.listener(|_, _, window, _cx| {
                                                window.remove_window();
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .track_focus(&self.copy_entry.focus_handle)
                                    .on_action(cx.listener(
                                        |this, _: &menu::Confirm, window, cx| {
                                            this.copy_details(window, cx);
                                        },
                                    ))
                                    .child(
                                        Button::new("copy", "Copy")
                                            .full_width()
                                            .style(ButtonStyle::Tinted(TintColor::Accent))
                                            .toggle_state(copy_is_focused)
                                            .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                                            .on_click(cx.listener(|this, _event, window, cx| {
                                                this.copy_details(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .into_any_element(),
            )
            .entry(self.ok_entry.clone())
            .entry(self.copy_entry.clone())
        }
    }

    impl Focusable for AboutWindow {
        fn focus_handle(&self, _cx: &App) -> FocusHandle {
            self.ok_entry.focus_handle.clone()
        }
    }

    // Don't open about window twice
    if let Some(existing) = cx
        .windows()
        .into_iter()
        .find_map(|w| w.downcast::<AboutWindow>())
    {
        existing
            .update(cx, |about_window, window, cx| {
                window.activate_window();
                about_window.ok_entry.focus_handle.focus(window, cx);
            })
            .log_err();
        return;
    }

    let window_size = Size {
        width: px(440.),
        height: px(300.),
    };

    cx.open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("About Zed".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.), px(12.))),
            }),
            window_bounds: Some(WindowBounds::centered(window_size, cx)),
            is_resizable: false,
            is_minimizable: false,
            kind: WindowKind::Floating,
            app_id: Some(settings_content::app_metadata::APP_ID.to_owned()),
            ..Default::default()
        },
        |window, cx| {
            let about_window = cx.new(AboutWindow::new);
            let focus_handle = about_window.read(cx).ok_entry.focus_handle.clone();
            window.activate_window();
            focus_handle.focus(window, cx);
            about_window
        },
    )
    .log_err();
}

static WAITING_QUIT_CONFIRMATION: AtomicBool = AtomicBool::new(false);
fn quit(_: &Quit, cx: &mut App) {
    if WAITING_QUIT_CONFIRMATION.load(atomic::Ordering::Acquire) {
        return;
    }

    let should_confirm = WorkspaceSettings::get_global(cx).confirm_quit;
    cx.spawn(async move |cx| {
        let mut workspace_windows: Vec<WindowHandle<MultiWorkspace>> = cx.update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<MultiWorkspace>())
                .collect::<Vec<_>>()
        });

        // If multiple windows have unsaved changes, and need a save prompt,
        // prompt in the active window before switching to a different window.
        cx.update(|cx| {
            workspace_windows.sort_by_key(|window| window.is_active(cx) == Some(false));
        });

        if should_confirm && let Some(multi_workspace) = workspace_windows.first() {
            let answer = multi_workspace
                .update(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Info,
                        "Are you sure you want to quit?",
                        None,
                        &["Quit", "Cancel"],
                        cx,
                    )
                })
                .log_err();

            if let Some(answer) = answer {
                WAITING_QUIT_CONFIRMATION.store(true, atomic::Ordering::Release);
                let answer = answer.await.ok();
                WAITING_QUIT_CONFIRMATION.store(false, atomic::Ordering::Release);
                if answer != Some(0) {
                    return Ok(());
                }
            }
        }

        if !workspace::prepare_windows_to_quit(&workspace_windows, cx).await {
            return Ok(());
        }

        cx.update(|cx| cx.quit());
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

fn open_log_file(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    const MAX_LINES: usize = 1000;
    let app_state = workspace.app_state();
    let languages = app_state.languages.clone();
    let fs = app_state.fs.clone();
    cx.spawn_in(window, async move |workspace, cx| {
        let log = {
            let result = futures::join!(
                fs.load(&paths::old_log_file()),
                fs.load(&paths::log_file()),
                languages.language_for_name("log")
            );
            match result {
                (Err(_), Err(e), _) => Err(e),
                (old_log, new_log, lang) => {
                    let mut lines = VecDeque::with_capacity(MAX_LINES);
                    for line in old_log
                        .iter()
                        .flat_map(|log| log.lines())
                        .chain(new_log.iter().flat_map(|log| log.lines()))
                    {
                        if lines.len() == MAX_LINES {
                            lines.pop_front();
                        }
                        lines.push_back(line);
                    }
                    Ok((
                        lines
                            .into_iter()
                            .flat_map(|line| [line, "\n"])
                            .collect::<String>(),
                        lang.ok(),
                    ))
                }
            }
        };

        let (log, log_language) = match log {
            Ok((log, log_language)) => (log, log_language),
            Err(e) => {
                struct OpenLogError;

                workspace
                    .update(cx, |workspace, cx| {
                        workspace.show_notification(
                            NotificationId::unique::<OpenLogError>(),
                            cx,
                            |cx| {
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
                            },
                        );
                    })
                    .ok();
                return;
            }
        };
        maybe!(async move {
            let project = workspace
                .read_with(cx, |workspace, _| workspace.project().clone())
                .ok()?;
            let buffer = project
                .update(cx, |project, cx| {
                    project.create_buffer(log_language, false, cx)
                })
                .await
                .ok()?;
            buffer.update(cx, |buffer, cx| {
                buffer.set_capability(Capability::ReadOnly, cx);
                buffer.set_text(log, cx);
            });

            let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx).with_title("Log".into()));

            let editor = cx
                .new_window_entity(|window, cx| {
                    let mut editor = Editor::for_multibuffer(buffer, Some(project), window, cx);
                    editor.set_read_only(true);
                    editor.set_breadcrumb_header(format!(
                        "Last {} lines in {}",
                        MAX_LINES,
                        paths::log_file().display()
                    ));
                    let last_multi_buffer_offset = editor.buffer().read(cx).len(cx);
                    editor.change_selections(Default::default(), window, cx, |s| {
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
        })
        .await;
    })
    .detach();
}

fn init_global_config_error_notifications(cx: &mut App) {
    cx.observe_new(|_: &mut SettingsObserver, _, cx| {
        cx.subscribe_self::<SettingsObserverEvent>(|_, event, cx| {
            let (result, file_kind, on_click): (_, _, fn(&mut Window, &mut App)) = match event {
                SettingsObserverEvent::GlobalTasksUpdated(result) => {
                    (result, "tasks", |window, cx| {
                        window.dispatch_action(OpenTasks.boxed_clone(), cx)
                    })
                }
                _ => return,
            };
            let id = NotificationId::Named(format!("invalid-global-{file_kind}-file").into());
            match result {
                Ok(_) => dismiss_app_notification(&id, cx),
                Err(error) => {
                    let message = format!("Invalid global {file_kind} file\n{error}");
                    show_app_notification(id, cx, move |cx| {
                        cx.new(|cx| {
                            MessageNotification::new(message.clone(), cx)
                                .primary_message("Open File")
                                .primary_icon(IconName::Settings)
                                .primary_on_click(move |window, cx| {
                                    on_click(window, cx);
                                    cx.emit(DismissEvent);
                                })
                        })
                    });
                }
            }
        })
        .detach();
    })
    .detach();
}

#[derive(Copy, Clone, Debug, settings::RegisterSetting)]
struct CursorHideModeSetting(gpui::CursorHideMode);

impl Settings for CursorHideModeSetting {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        Self(match content.hide_mouse.unwrap_or_default() {
            settings::HideMouseMode::Never => gpui::CursorHideMode::Never,
            settings::HideMouseMode::OnTyping => gpui::CursorHideMode::OnTyping,
            settings::HideMouseMode::OnTypingAndAction => gpui::CursorHideMode::OnTypingAndAction,
        })
    }
}

fn init_cursor_hide_mode(cx: &mut App) {
    let apply = |cx: &mut App| cx.set_cursor_hide_mode(CursorHideModeSetting::get_global(cx).0);
    apply(cx);
    cx.observe_global::<SettingsStore>(apply).detach();
}

fn init_app_appearance(cx: &mut App) {
    // Force the native window chrome (border + titlebar) to match the selected theme.
    // `System` follows the OS (no override); any other theme forces its appearance, so a
    // dark theme doesn't render a light window border when the system is in light mode.
    let apply = |cx: &mut App| {
        let appearance = match ThemeSettings::get_global(cx).theme.mode() {
            Some(theme_settings::ThemeAppearanceMode::System) => None,
            _ => Some(match cx.theme().appearance() {
                theme::Appearance::Light => gpui::WindowAppearance::Light,
                theme::Appearance::Dark => gpui::WindowAppearance::Dark,
            }),
        };
        cx.set_window_appearance(appearance);
    };
    apply(cx);
    cx.observe_global::<SettingsStore>(apply).detach();
}

#[derive(Copy, Clone, Debug, settings::RegisterSetting)]
struct ReduceMotionSetting(settings::ReduceMotionMode);

impl Settings for ReduceMotionSetting {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        Self(content.reduce_motion.unwrap_or_default())
    }
}

fn init_reduce_motion(cx: &mut App) {
    let apply = |cx: &mut App| {
        let reduce_motion = ReduceMotionSetting::get_global(cx).0 == settings::ReduceMotionMode::On;
        cx.set_reduce_motion(reduce_motion);
    };
    apply(cx);
    cx.observe_global::<SettingsStore>(apply).detach();
}

pub fn initialize_keymap(cx: &mut App) {
    reload_keymap(cx);
    cx.on_keyboard_layout_change(reload_keymap).detach();
}

fn reload_keymap(cx: &mut App) {
    cx.clear_key_bindings();
    let bindings = settings::built_in_key_bindings(cx);
    cx.bind_keys(bindings);
    let menus = app_menus(cx);
    cx.set_menus(menus);
    cx.set_dock_menu(vec![gpui::MenuItem::action(
        "New Window",
        workspace::NewWindow,
    )]);
}

/// Populates a freshly opened window according to the `on_new_window` setting.
///
/// For `EmptyTab` this opens an empty buffer. For `Launchpad` the window is left
/// without any items so the pane falls back to displaying the Launchpad.
fn initialize_new_window(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if WorkspaceSettings::get_global(cx).on_new_window == settings::OnNewWindow::Launchpad {
        return;
    }
    // Create buffer synchronously to avoid flicker
    let project = workspace.project().clone();
    let buffer = project.update(cx, |project, cx| {
        project.create_local_buffer("", None, true, cx)
    });
    buffer.update(cx, |buffer, _| {
        buffer.set_content_language_detection_enabled(true);
    });
    let editor = cx.new(|cx| Editor::for_buffer(buffer, Some(project), window, cx));
    workspace.add_item_to_active_pane(Box::new(editor), None, true, window, cx);
}

pub fn open_new_ssh_project_from_project(
    workspace: &mut Workspace,
    paths: Vec<PathBuf>,
    create_new_window: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Task<anyhow::Result<()>> {
    let app_state = workspace.app_state().clone();
    let Some(ssh_client) = workspace.project().read(cx).remote_client() else {
        return Task::ready(Err(anyhow::anyhow!("Not an ssh project")));
    };
    let connection_options = ssh_client.read(cx).connection_options();
    let requesting_window = if create_new_window {
        None
    } else {
        window.window_handle().downcast::<MultiWorkspace>()
    };
    cx.spawn_in(window, async move |_, cx| {
        open_remote_project(
            connection_options,
            paths,
            app_state,
            workspace::OpenOptions {
                workspace_matching: workspace::WorkspaceMatching::None,
                requesting_window,
                ..Default::default()
            },
            cx,
        )
        .await
        .map(|_| ())
    })
}

fn open_project_tasks_file(
    workspace: &mut Workspace,
    _: &OpenProjectTasks,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(task) = open_local_file(
        workspace,
        local_tasks_file_relative_path(),
        initial_tasks_content(),
        window,
        cx,
    ) {
        task.detach_and_log_err(cx);
    }
}

fn open_worktree_setup_tasks_file(
    workspace: &mut Workspace,
    _: &zed_actions::OpenWorktreeSetupTasks,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // Kept harmless on purpose: tasks with the `create_worktree` hook run automatically
    // when a worktree is created, so the example must be safe to save unedited.
    const WORKTREE_SETUP_TASK_EXAMPLE: &str = r#"  {
    // Runs automatically after Zed creates a new git worktree.
    // $ZED_WORKTREE_ROOT is the new worktree's root directory, and
    // $ZED_MAIN_GIT_WORKTREE is the original repository's working directory.
    "label": "Set up new worktree",
    "command": "echo \"Setting up $ZED_WORKTREE_ROOT — edit this command\"",
    "cwd": "$ZED_WORKTREE_ROOT",
    "hooks": ["create_worktree"]
  }"#;

    let Some(open_task) = open_local_file(
        workspace,
        local_tasks_file_relative_path(),
        settings::initial_worktree_setup_tasks_content(),
        window,
        cx,
    ) else {
        return;
    };

    cx.spawn_in(window, async move |_, cx| {
        let editor = open_task.await?;
        editor.update_in(cx, |editor, window, cx| {
            // Skip insertion if the file already mentions the hook (even in a comment,
            // like the seeded template's example — uncommenting it beats duplicating it).
            // `create_git_worktree` is a serde alias for the same hook.
            let text = editor.text(cx);
            if text.contains("create_worktree") || text.contains("create_git_worktree") {
                return anyhow::Ok(());
            }
            tasks_ui::insert_task_json_into_editor(
                editor,
                WORKTREE_SETUP_TASK_EXAMPLE.to_string(),
                window,
                cx,
            )
        })?
    })
    .detach_and_log_err(cx);
}

fn open_local_file(
    workspace: &mut Workspace,
    settings_relative_path: &'static RelPath,
    initial_contents: Cow<'static, str>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Option<gpui::Task<anyhow::Result<Entity<Editor>>>> {
    let project = workspace.project().clone();
    let worktree = project
        .read(cx)
        .visible_worktrees(cx)
        .find_map(|tree| tree.read(cx).root_entry()?.is_dir().then_some(tree));
    if let Some(worktree) = worktree {
        let tree_id = worktree.read(cx).id();
        Some(cx.spawn_in(window, async move |workspace, cx| {
            // Check if the file actually exists on disk (even if it's excluded from worktree)
            let file_exists = {
                let full_path = worktree.read_with(cx, |tree, _| {
                    tree.abs_path().join(settings_relative_path.as_std_path())
                });

                let fs = project.read_with(cx, |project, _| project.fs().clone());

                fs.metadata(&full_path)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|metadata| !metadata.is_dir && !metadata.is_fifo)
            };

            if !file_exists {
                if let Some(dir_path) = settings_relative_path.parent()
                    && worktree.read_with(cx, |tree, _| tree.entry_for_path(dir_path).is_none())
                {
                    project
                        .update(cx, |project, cx| {
                            project.create_entry((tree_id, dir_path), true, cx)
                        })
                        .await
                        .context("worktree was removed")?;
                }

                if worktree.read_with(cx, |tree, _| {
                    tree.entry_for_path(settings_relative_path).is_none()
                }) {
                    project
                        .update(cx, |project, cx| {
                            project.create_entry((tree_id, settings_relative_path), false, cx)
                        })
                        .await
                        .context("worktree was removed")?;
                }
            }

            let editor = workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.open_path((tree_id, settings_relative_path), None, true, window, cx)
                })?
                .await?
                .downcast::<Editor>()
                .context("unexpected item type: expected editor item")?;

            editor.update(cx, |editor, cx| {
                if let Some(buffer) = editor.buffer().read(cx).as_singleton()
                    && buffer.read(cx).is_empty()
                {
                    buffer.update(cx, |buffer, cx| {
                        buffer.edit([(0..0, initial_contents)], None, cx)
                    });
                }
            });

            anyhow::Ok(editor)
        }))
    } else {
        struct NoOpenFolders;

        workspace.show_notification(NotificationId::unique::<NoOpenFolders>(), cx, |cx| {
            cx.new(|cx| MessageNotification::new("This project has no folders open.", cx))
        });
        None
    }
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
                && editor
                    .buffer()
                    .read(cx)
                    .as_singleton()
                    .is_some_and(|buffer| buffer.read(cx).file().is_none())
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
                let buffer = project.update(cx, move |project, cx| {
                    project.create_buffer(language, false, cx)
                });
                cx.spawn_in(window, async move |workspace, cx| {
                    let buffer = buffer.await?;
                    buffer.update(cx, |buffer, cx| {
                        buffer.set_text(text.into_owned(), cx);
                        buffer.set_capability(Capability::ReadOnly, cx);
                    });
                    let buffer =
                        cx.new(|cx| MultiBuffer::singleton(buffer, cx).with_title(title.into()));
                    workspace.update_in(cx, |workspace, window, cx| {
                        workspace.add_item_to_active_pane(
                            Box::new(cx.new(|cx| {
                                let mut editor = Editor::for_multibuffer(
                                    buffer,
                                    Some(project.clone()),
                                    window,
                                    cx,
                                );
                                editor.set_read_only(true);
                                editor.set_should_serialize(false, cx);
                                editor.set_breadcrumb_header(title.into());
                                editor
                            })),
                            None,
                            true,
                            window,
                            cx,
                        )
                    })
                })
            })?
            .await
    })
    .detach_and_log_err(cx);
}

fn open_settings_file(
    abs_path: &'static Path,
    default_content: impl FnOnce() -> Rope + Send + 'static,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    cx.spawn_in(window, async move |workspace, cx| {
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.with_local_workspace(window, cx, move |workspace, window, cx| {
                    let project = workspace.project().clone();

                    cx.spawn_in(window, async move |workspace, cx| {
                        // Set up a dedicated worktree for settings, since
                        // otherwise we're dropping and re-starting LSP servers
                        // for each file inside on every settings file
                        // close/open

                        // TODO: Do note that all other external files (e.g.
                        // drag and drop from OS) still have their worktrees
                        // released on file close, causing LSP servers'
                        // restarts.
                        let (_worktree, _) = project
                            .update(cx, |project, cx| {
                                project.find_or_create_worktree(paths::config_dir(), false, cx)
                            })
                            .await?;

                        workspace
                            .update_in(cx, |_, window, cx| {
                                create_and_open_local_file(abs_path, window, cx, default_content)
                            })?
                            .await?;
                        anyhow::Ok(())
                    })
                })
            })?
            .await?
            .await?;
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}
