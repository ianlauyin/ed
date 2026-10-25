#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod zed;

// Ensure the binary name stays in sync with APP_NAME so that the paths used
// at runtime (data dir, config dir, etc.) match what the binary is called.
const _: () = assert!(
    paths::APP_NAME.as_bytes().eq_ignore_ascii_case(env!("CARGO_BIN_NAME").as_bytes()),
    "paths::APP_NAME must match the binary name. \
     Update APP_NAME in crates/paths/src/paths.rs when renaming the binary.",
);

use std::cell::RefCell;
use std::env;
use std::io::IsTerminal;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use assets::Assets;
use clap::Parser;
use collections::HashMap;
use db::kvp::KeyValueStore;
use editor::Editor;
use fs::Fs;
use fs::RealFs;
use futures::FutureExt;
use futures::StreamExt;
use futures::channel::oneshot;
use gpui::App;
use gpui::AppContext;
use gpui::Application;
use gpui::AsyncApp;
use gpui::QuitMode;
use gpui::Task;
use gpui::TaskExt;
use gpui::UpdateGlobal as _;
use language::LanguageRegistry;
use project::trusted_worktrees;
use recent_projects::RemoteSettings;
use recent_projects::open_remote_project;
use remote::RemoteConnectionOptions;
use session::AppSession;
use session::Session;
use settings::Settings;
use settings::SettingsStore;
use theme::ActiveTheme;
use theme::GlobalTheme;
use util::ResultExt;
use uuid::Uuid;
use workspace::AppState;
use workspace::MultiWorkspace;
use workspace::SerializedWorkspaceLocation;
use workspace::SessionWorkspace;
use workspace::Toast;
use workspace::WorkspaceSettings;
use workspace::WorkspaceStore;
use workspace::notifications::NotificationId;
use workspace::restore_multiworkspace;
use zed::OpenListener;
use zed::OpenRequest;
use zed::RawOpenRequest;
use zed::build_window_options;
use zed::derive_paths_with_position;
use zed::initialize_keymap;
use zed::initialize_workspace;
use zed::open_paths_with_positions;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn build_application() -> Application {
    Application::with_platform(gpui_platform::current_platform(false)).with_quit_mode(QuitMode::LastWindowClosed)
}

fn files_not_created_on_launch(errors: HashMap<io::ErrorKind, Vec<&Path>>) {
    let message = "ed failed to launch";
    let error_details = errors
        .into_iter()
        .filter_map(|(kind, paths)| {
            #[allow(unused_mut)] // for non-unix platforms
            let mut error_kind_details = match paths.len() {
                0 => return None,
                1 => format!(
                    "{kind} when creating directory {:?}",
                    paths.first().expect("match arm checks for a single entry")
                ),
                _many => format!("{kind} when creating directories {paths:?}"),
            };


            {
                if kind == io::ErrorKind::PermissionDenied {
                    error_kind_details.push_str("\n\nConsider using chown and chmod tools for altering the directories permissions if your user has corresponding rights.\
                        \nFor example, `sudo chown $(whoami):staff ~/.config` and `chmod +uwrx ~/.config`");
                }
            }

            Some(error_kind_details)
        })
        .collect::<Vec<_>>().join("\n\n");

    eprintln!("{message}: {error_details}");
    build_application().with_quit_mode(QuitMode::Explicit).run(move |cx| {
        if let Ok(window) = cx.open_window(gpui::WindowOptions::default(), |_, cx| cx.new(|_| gpui::Empty)) {
            window
                .update(cx, |_, window, cx| {
                    let response =
                        window.prompt(gpui::PromptLevel::Critical, message, Some(&error_details), &["Exit"], cx);

                    cx.spawn_in(window, async move |_, cx| {
                        response.await?;
                        cx.update(|_, cx| cx.quit())
                    })
                    .detach_and_log_err(cx);
                })
                .log_err();
        } else {
            fail_to_open_window(anyhow::anyhow!("{message}: {error_details}"), cx)
        }
    })
}

fn fail_to_open_window_async(e: anyhow::Error, cx: &AsyncApp) {
    cx.update(|cx| fail_to_open_window(e, cx));
}

fn fail_to_open_window(e: anyhow::Error, _cx: &mut App) {
    zlog::error!("Failed to open window: {e:?}");
    eprintln!("ed failed to open a window: {e:?}.");
    process::exit(1);
}

fn main() {
    util::prevent_root_execution();

    let args = Args::parse();

    // `zed --printenv` Outputs environment variables as JSON to stdout
    if args.printenv {
        util::shell_env::print_env();
        return;
    }

    let restart_arguments = if let Some(directory) = args.user_data_dir.as_deref() {
        let directory = paths::set_custom_data_dir(directory);
        vec![std::ffi::OsString::from("--user-data-dir"), directory.as_os_str().to_owned()]
    } else {
        Vec::new()
    };

    let file_errors = init_paths();
    if !file_errors.is_empty() {
        files_not_created_on_launch(file_errors);
        return;
    }

    zlog::init();

    if stdout_is_a_pty() {
        zlog::init_output_stdout();
    } else {
        let result = zlog::init_output_file(paths::log_file(), Some(paths::old_log_file()));
        if let Err(err) = result {
            eprintln!("Could not open log file: {}... Defaulting to stdout", err);
            zlog::init_output_stdout();
        };
    }
    install_panic_hook();

    util::increase_open_file_limit().log_err();

    rayon::ThreadPoolBuilder::new()
        .num_threads(std::thread::available_parallelism().map_or(1, |n| n.get().div_ceil(2)))
        .stack_size(10 * 1024 * 1024)
        .thread_name(|ix| format!("RayonWorker{}", ix))
        .build_global()
        .unwrap();

    log::info!("========== starting zed version {} ==========", env!("CARGO_PKG_VERSION"));

    let app = build_application().with_assets(Assets).with_restart_arguments(restart_arguments);

    let app_db = db::AppDatabase::new();

    let session_id = Uuid::new_v4().to_string();
    let session = app.background_executor().spawn(Session::new(session_id, KeyValueStore::from_app_db(&app_db)));
    let (open_listener, mut open_rx) = OpenListener::new();

    let failed_single_instance_check = if cfg!(debug_assertions) {
        false
    } else {
        {
            use zed::mac_only_instance::*;
            ensure_only_instance() != IsOnlyInstance::Yes
        }
    };
    if failed_single_instance_check {
        println!("ed is already running");
        return;
    }

    let fs = RealFs::new(app.background_executor());
    if !stdout_is_a_pty() {
        app.background_executor()
            .spawn(async {
                util::load_login_shell_environment().await.log_err();
            })
            .detach();
    }

    app.on_open_urls({
        let open_listener = open_listener.clone();
        move |urls| open_listener.open(RawOpenRequest { urls, diff_paths: Vec::new(), ..Default::default() })
    });
    app.on_reopen(move |cx| {
        if let Some(app_state) = AppState::try_global(cx) {
            cx.spawn({
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx)
                    }
                }
            })
            .detach();
        }
    });

    app.run(move |cx| {
        cx.set_global(app_db);
        let db_trusted_paths = match workspace::WorkspaceDb::global(cx).fetch_trusted_worktrees() {
            Ok(trusted_paths) => trusted_paths,
            Err(e) => {
                log::error!("Failed to do initial trusted worktrees fetch: {e:#}");
                HashMap::default()
            }
        };
        trusted_worktrees::init(db_trusted_paths, cx);
        menu::init();
        zed_actions::init();

        settings::init(cx);
        zlog_settings::init(cx);
        initialize_keymap(cx);

        <dyn Fs>::set_global(fs.clone(), cx);

        OpenListener::set_global(cx, open_listener.clone());

        let languages = Arc::new(LanguageRegistry::new(cx.background_executor().clone()));
        ui::on_new_scrollbars::<SettingsStore>(cx);

        languages::init(languages.clone(), fs.clone(), cx);

        let workspace_store = cx.new(WorkspaceStore::new);

        zed::init(cx);

        zed::move_to_applications::init(cx);

        let session = cx.foreground_executor().block_on(session);

        let app_session = cx.new(|cx| AppSession::new(session, cx));

        let app_state = Arc::new(AppState {
            languages,

            fs: fs.clone(),
            build_window_options,
            workspace_store,
            session: app_session,
        });
        AppState::set_global(app_state.clone(), cx);

        theme_settings::init(theme::LoadThemes::All(Box::new(Assets)), cx);
        command_palette::init(cx);

        recent_projects::init(cx);

        editor::init(cx);
        image_viewer::init(cx);
        diagnostics::init(cx);

        workspace::init(app_state.clone(), cx);

        go_to_line::init(cx);
        file_finder::init(cx);
        tab_switcher::init(cx);
        outline::init(cx);
        call_hierarchy::init(cx);
        project_symbols::init(cx);
        project_panel::init(cx);
        outline_panel::init(cx);

        search::init(cx);
        lsp_locations::init(cx);
        cx.set_global(workspace::PaneSearchBarCallbacks {
            setup_search_bar: |toolbar, window, cx| {
                let search_bar = cx.new(|cx| search::BufferSearchBar::new(window, cx));
                toolbar.update(cx, |toolbar, cx| {
                    toolbar.add_item(search_bar, window, cx);
                });
            },
            wrap_div_with_search_actions: search::buffer_search::register_pane_search_actions,
        });
        terminal_view::init(cx);
        encoding_selector::init(cx);
        language_selector::init(cx);
        lsp_command_selector::init(cx);
        theme_selector::init(cx);
        language_tools::init(cx);

        title_bar::init(cx);
        git_ui::init(cx);
        markdown_preview::init(cx);
        tabular_data_preview::init(cx);
        svg_preview::init(cx);
        json_schema_store::init(cx);

        cx.observe_global::<SettingsStore>({
            move |cx| {
                for &mut window in &mut cx.windows() {
                    let background_appearance = cx.theme().window_background_appearance();
                    window.update(cx, |_, window, _| window.set_background_appearance(background_appearance)).ok();
                }

                cx.set_text_rendering_mode(match WorkspaceSettings::get_global(cx).text_rendering_mode {
                    settings::TextRenderingMode::PlatformDefault => gpui::TextRenderingMode::PlatformDefault,
                    settings::TextRenderingMode::Subpixel => gpui::TextRenderingMode::Subpixel,
                    settings::TextRenderingMode::Grayscale => gpui::TextRenderingMode::Grayscale,
                });
            }
        })
        .detach();
        app_state.languages.set_theme(cx.theme().clone());
        cx.observe_global::<GlobalTheme>({
            let languages = app_state.languages.clone();
            move |cx| {
                languages.set_theme(cx.theme().clone());
            }
        })
        .detach();

        #[cfg(debug_assertions)]
        watch_languages(app_state.fs.clone(), app_state.languages.clone(), cx);

        initialize_workspace(app_state.clone(), cx);

        cx.activate(true);

        let urls: Vec<_> = args.paths_or_urls.iter().map(|arg| parse_url_arg(arg, cx)).collect();

        // Check if any diff paths are directories to determine diff_all mode
        let diff_all_mode =
            args.diff.chunks(2).any(|pair| Path::new(&pair[0]).is_dir() || Path::new(&pair[1]).is_dir());

        let diff_paths: Vec<[String; 2]> =
            args.diff.chunks(2).map(|chunk| [chunk[0].clone(), chunk[1].clone()]).collect();

        if !urls.is_empty() || !diff_paths.is_empty() {
            open_listener.open(RawOpenRequest { urls, diff_paths, diff_all: diff_all_mode })
        }

        let (current_session_id, last_session_id) = {
            let session = app_state.session.read(cx);
            (session.id().to_owned(), session.last_session_id().map(|id| id.to_owned()))
        };

        let restore_task = match open_rx.try_recv().ok().and_then(|request| OpenRequest::parse(request, cx).log_err()) {
            Some(request) => {
                handle_open_request(request, app_state.clone(), cx);
                Task::ready(())
            }
            None => cx.spawn({
                let app_state = app_state.clone();
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx)
                    }
                }
            }),
        };

        let (first_window_tx, first_window_rx) = oneshot::channel::<()>();
        let first_window_tx = Rc::new(RefCell::new(Some(first_window_tx)));
        let _first_window_subscription = cx.observe_new::<MultiWorkspace>(move |_, _, _| {
            if let Some(tx) = first_window_tx.borrow_mut().take() {
                tx.send(()).ok();
            }
        });

        let restore_finished = cx.background_spawn(restore_task).shared();

        cx.spawn({
            let db = workspace::WorkspaceDb::global(cx);
            let fs = app_state.fs.clone();
            let restore_finished = restore_finished.clone();
            async move |_cx| {
                restore_finished.await;
                db.garbage_collect_workspaces(fs.as_ref(), &current_session_id, last_session_id.as_deref()).await
            }
        })
        .detach_and_log_err(cx);

        let app_state = app_state;

        cx.spawn(async move |cx| {
            let _first_window_subscription = _first_window_subscription;
            let first_window_placed = first_window_rx.shared();
            while let Some(urls) = open_rx.next().await {
                // On a macOS cold launch, `zed <path>` arrives here after startup already
                // began restoring the session, so wait for a restored window to exist before
                // matching. Otherwise this open sees no windows and spawns a redundant one (#61346).
                futures::select_biased! {
                    _ = restore_finished.clone() => {}
                    _ = first_window_placed.clone() => {}
                }
                cx.update(|cx| {
                    if let Some(request) = OpenRequest::parse(urls, cx).log_err() {
                        handle_open_request(request, app_state.clone(), cx);
                    }
                });
            }
        })
        .detach();
    });
}

fn handle_open_request(request: OpenRequest, app_state: Arc<AppState>, cx: &App) {
    if let Some(connection_options) = request.remote_connection {
        let base_open_options = zed::open_options_for_request(cx);
        cx.spawn(async move |cx| {
            let paths: Vec<PathBuf> = request.open_paths.into_iter().map(PathBuf::from).collect();
            open_remote_project(connection_options, paths, app_state, base_open_options, cx).await
        })
        .detach_and_log_err(cx);
        return;
    }

    let mut task = None;
    if !request.open_paths.is_empty() || !request.diff_paths.is_empty() {
        let base_open_options = zed::open_options_for_request(cx);
        task = Some(cx.spawn(async move |cx| {
            let paths_with_position = derive_paths_with_position(app_state.fs.as_ref(), request.open_paths).await;
            let (_window, results) = open_paths_with_positions(
                &paths_with_position,
                &request.diff_paths,
                request.diff_all,
                app_state,
                base_open_options,
                cx,
            )
            .await?;
            for result in results.into_iter().flatten() {
                if let Err(err) = result {
                    log::error!("Error opening path: {err:#}");
                }
            }
            anyhow::Ok(())
        }));
    }

    if let Some(task) = task {
        cx.spawn(async move |cx| {
            if let Err(err) = task.await {
                fail_to_open_window_async(err, cx);
            }
        })
        .detach();
    }
}

pub(crate) async fn restore_or_create_workspace(app_state: Arc<AppState>, cx: &mut AsyncApp) -> Result<()> {
    if let Some(multi_workspaces) = restorable_workspaces(cx, &app_state).await {
        let mut error_count = 0;
        for multi_workspace in multi_workspaces {
            let result = match &multi_workspace.active_workspace.location {
                SerializedWorkspaceLocation::Local => {
                    restore_multiworkspace(multi_workspace, app_state.clone(), cx).await.map(|_| ())
                }
                SerializedWorkspaceLocation::Remote(connection_options) => {
                    let mut connection_options = connection_options.clone();
                    let RemoteConnectionOptions::Ssh(options) = &mut connection_options;
                    cx.update(|cx| RemoteSettings::get_global(cx).fill_connection_options_from_settings(options));

                    let paths =
                        multi_workspace.active_workspace.paths.paths().iter().map(PathBuf::from).collect::<Vec<_>>();
                    let state = multi_workspace.state.clone();
                    async {
                        let window = open_remote_project(
                            connection_options,
                            paths,
                            app_state.clone(),
                            workspace::OpenOptions::default(),
                            cx,
                        )
                        .await?;
                        workspace::apply_restored_multiworkspace_state(window, &state, app_state.fs.clone(), cx).await;
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                }
            };

            if let Err(error) = result {
                log::error!("Failed to restore workspace: {error:#}");
                error_count += 1;
            }
        }

        if error_count > 0 {
            let message = if error_count == 1 {
                "Failed to restore 1 workspace. Check logs for details.".to_string()
            } else {
                format!("Failed to restore {} workspaces. Check logs for details.", error_count)
            };

            // Try to find an active workspace to show the toast
            let toast_shown = cx.update(|cx| {
                if let Some(window) = cx.active_window()
                    && let Some(multi_workspace) = window.downcast::<MultiWorkspace>()
                {
                    multi_workspace
                        .update(cx, |multi_workspace, _, cx| {
                            multi_workspace.workspace().update(cx, |workspace, cx| {
                                workspace.show_toast(Toast::new(NotificationId::unique::<()>(), message.clone()), cx)
                            });
                        })
                        .ok();
                    return true;
                }
                false
            });

            // If we couldn't show a toast (no windows opened successfully),
            // open a fallback empty workspace and show the error there
            if !toast_shown {
                log::error!("All workspace restorations failed. Opening fallback empty workspace.");
                cx.update(|cx| {
                    workspace::open_new(
                        workspace::OpenOptions::default(),
                        app_state.clone(),
                        cx,
                        |workspace, _window, cx| {
                            workspace.show_toast(Toast::new(NotificationId::unique::<()>(), message), cx);
                        },
                    )
                })
                .await?;
            }
        }

        // If the user cancelled a failed remote connection at startup,
        // open_remote_project returns Ok but removes the window, so error_count
        // stays 0 and the toast fallback above does not trigger. Without this
        // check, Zed would exit silently.
        if cx.update(|cx| cx.windows().is_empty()) {
            cx.update(|cx| {
                workspace::open_new(
                    workspace::OpenOptions::default(),
                    app_state.clone(),
                    cx,
                    |workspace, window, cx| {
                        let restore_on_startup = WorkspaceSettings::get_global(cx).restore_on_startup;
                        match restore_on_startup {
                            workspace::RestoreOnStartupBehavior::Launchpad => {}
                            _ => {
                                Editor::new_file(workspace, &workspace::NewFile, window, cx);
                            }
                        }
                    },
                )
            })
            .await?;
        }
    } else {
        cx.update(|cx| {
            workspace::open_new(workspace::OpenOptions::default(), app_state, cx, |workspace, window, cx| {
                let restore_on_startup = WorkspaceSettings::get_global(cx).restore_on_startup;
                match restore_on_startup {
                    workspace::RestoreOnStartupBehavior::Launchpad => {}
                    _ => {
                        Editor::new_file(workspace, &workspace::NewFile, window, cx);
                    }
                }
            })
        })
        .await?;
    }

    Ok(())
}

async fn restorable_workspaces(
    cx: &AsyncApp,
    app_state: &Arc<AppState>,
) -> Option<Vec<workspace::SerializedMultiWorkspace>> {
    let locations = restorable_workspace_locations(cx, app_state).await?;
    Some(cx.update(|cx| workspace::read_serialized_multi_workspaces(locations, cx)))
}

pub(crate) async fn restorable_workspace_locations(
    cx: &AsyncApp,
    app_state: &Arc<AppState>,
) -> Option<Vec<SessionWorkspace>> {
    let (mut restore_behavior, db) =
        cx.update(|cx| (WorkspaceSettings::get(None, cx).restore_on_startup, workspace::WorkspaceDb::global(cx)));

    let session_handle = app_state.session.clone();
    let (last_session_id, last_session_window_stack) = cx.update(|cx| {
        let session = session_handle.read(cx);

        (session.last_session_id().map(|id| id.to_string()), session.last_session_window_stack())
    });

    if last_session_id.is_none() && matches!(restore_behavior, workspace::RestoreOnStartupBehavior::LastSession) {
        restore_behavior = workspace::RestoreOnStartupBehavior::LastWorkspace;
    }

    match restore_behavior {
        workspace::RestoreOnStartupBehavior::LastWorkspace => {
            workspace::last_opened_workspace_location(&db, app_state.fs.as_ref()).await.map(
                |(workspace_id, location, paths)| {
                    vec![SessionWorkspace { workspace_id, location, paths, window_id: None }]
                },
            )
        }
        workspace::RestoreOnStartupBehavior::LastSession => {
            if let Some(last_session_id) = last_session_id {
                let ordered = last_session_window_stack.is_some();

                let mut locations = workspace::last_session_workspace_locations(
                    &db,
                    &last_session_id,
                    last_session_window_stack,
                    app_state.fs.as_ref(),
                )
                .await
                .filter(|locations| !locations.is_empty());

                // Since last_session_window_order returns the windows ordered front-to-back
                // we need to open the window that was frontmost last.
                if ordered && let Some(locations) = locations.as_mut() {
                    locations.reverse();
                }

                locations
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Logs panics with a backtrace, then exits so a panic on any thread takes the
/// whole process down (and the macOS crash dialog never shows).
fn install_panic_hook() {
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let message = info.payload_as_str().unwrap_or("Box<Any>");
        let location = info.location().map_or_else(|| "<unknown>".to_owned(), |location| location.to_string());
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        log::error!("thread '{thread_name}' panicked at {location}:\n{message}\n{backtrace}");
        zlog::flush();
        old_hook(info);
        std::process::exit(1);
    }));
}

fn init_paths() -> HashMap<io::ErrorKind, Vec<&'static Path>> {
    [paths::config_dir(), paths::database_dir(), paths::logs_dir(), paths::temp_dir()].into_iter().fold(
        HashMap::default(),
        |mut errors, path| {
            if let Err(e) = std::fs::create_dir_all(path) {
                errors.entry(e.kind()).or_insert_with(Vec::new).push(path);
            }
            errors
        },
    )
}

fn stdout_is_a_pty() -> bool {
    io::stdout().is_terminal()
}

#[derive(Parser, Debug)]
#[command(name = "zed", disable_version_flag = true, max_term_width = 100)]
struct Args {
    /// A sequence of space-separated paths or urls that you want to open.
    ///
    /// Use `path:line:row` syntax to open a file at a specific location.
    /// Non-existing paths and directories will ignore `:line:row` suffix.
    ///
    /// URLs can be `file://` or `ssh://` scheme.
    paths_or_urls: Vec<String>,

    /// Pairs of file paths to diff. Can be specified multiple times.
    /// When directories are provided, recurses into them and shows all changed files in a single multi-diff view.
    #[arg(long, action = clap::ArgAction::Append, num_args = 2, value_names = ["OLD_PATH", "NEW_PATH"])]
    diff: Vec<String>,

    /// Sets a custom directory for all user data (e.g., database, extensions, logs).
    ///
    /// This overrides the default data directory location, `$XDG_DATA_HOME/ed`
    /// (`~/.local/share/ed`).
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    user_data_dir: Option<String>,

    /// Instructs zed to run as a dev server on this machine. (not implemented)
    #[arg(long)]
    dev_server_token: Option<String>,

    /// Output current environment variables as JSON to stdout
    #[arg(long, hide = true)]
    printenv: bool,
}

fn parse_url_arg(arg: &str, _cx: &App) -> String {
    match std::fs::canonicalize(Path::new(&arg)) {
        Ok(path) => format!("file://{}", path.display()),
        Err(_) => {
            if arg.starts_with("file://") || arg.starts_with("ssh://") {
                arg.into()
            } else {
                format!("file://{arg}")
            }
        }
    }
}

#[cfg(debug_assertions)]
fn watch_languages(fs: Arc<dyn fs::Fs>, languages: Arc<LanguageRegistry>, cx: &App) {
    use std::time::Duration;

    cx.background_spawn(async move {
        let languages_src = Path::new("crates/grammars/src");
        let Some(languages_src) = fs.canonicalize(languages_src).await.log_err() else {
            return;
        };

        let (mut events, watcher) = fs.watch(&languages_src, Duration::from_millis(100)).await;

        // add subdirectories since fs.watch is not recursive on Linux
        if let Some(mut paths) = fs.read_dir(&languages_src).await.log_err() {
            while let Some(path) = paths.next().await {
                if let Some(path) = path.log_err()
                    && fs.is_dir(&path).await
                {
                    watcher.add(&path).log_err();
                }
            }
        }

        while let Some(event) = events.next().await {
            let has_language_file = event.iter().any(|event| event.path.extension().is_some_and(|ext| ext == "scm"));
            if has_language_file {
                languages.reload();
            }
        }
    })
    .detach();
}
