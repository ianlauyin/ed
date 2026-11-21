use std::backtrace::Backtrace;
use std::panic;
use std::thread;

use ed::control_server;
use search::buffer_search;
use util::shell_env;
mod ed;
mod logger;

// Ensure the binary name stays in sync with APP_NAME so that the paths used
// at runtime (data dir, config dir, etc.) match what the binary is called.
const _: () = assert!(
    paths::APP_NAME.as_bytes().eq_ignore_ascii_case(env!("CARGO_BIN_NAME").as_bytes()),
    "paths::APP_NAME must match the binary name. \
     Update APP_NAME in crates/paths/src/paths.rs when renaming the binary.",
);

use std::cell::RefCell;
use std::env;
use std::io;
use std::io::IsTerminal as _;
use std::path::Path;
use std::process;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use assets::Assets;
use clap::Parser;
use ed::build_window_options;
use ed::initialize_keymap;
use ed::initialize_workspace;
use ed::open_listener::OpenListener;
use ed::open_listener::OpenRequest;
use ed::open_listener::RawOpenRequest;
use ed::open_listener::derive_paths_with_position;
use ed::open_listener::open_paths_with_positions;
use editor::Editor;
use fs::Fs;
use fs::RealFs;
use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::channel::oneshot;
use gpui::App;
use gpui::AppContext as _;
use gpui::Application;
use gpui::AsyncApp;
use gpui::QuitMode;
use gpui::Task;
use gpui::TaskExt as _;
use gpui::UpdateGlobal as _;
use language::LanguageRegistry;
use rustc_hash::FxHashMap;
use theme::ActiveTheme as _;
use util::ResultExt as _;
use workspace::AppState;
use workspace::SessionWorkspace;
use workspace::Toast;
use workspace::Workspace;
use workspace::notifications::NotificationId;
use workspace::restore_workspace;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn build_application() -> Application {
    Application::with_platform(Rc::new(gpui_macos::MacPlatform::new(false))).with_quit_mode(QuitMode::LastWindowClosed)
}

#[expect(clippy::print_stderr, reason = "fatal startup error, the log may not be visible")]
fn files_not_created_on_launch(errors: FxHashMap<io::ErrorKind, Vec<&Path>>) {
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
            fail_to_open_window(anyhow::anyhow!("{message}: {error_details}"), cx);
        }
    });
}

fn fail_to_open_window_async(e: anyhow::Error, cx: &AsyncApp) {
    cx.update(|cx| fail_to_open_window(e, cx));
}

#[expect(clippy::print_stderr, reason = "fatal startup error, the log may not be visible")]
fn fail_to_open_window(e: anyhow::Error, _cx: &mut App) {
    log::error!("Failed to open window: {e:#}");
    eprintln!("ed failed to open a window: {e:#}.");
    process::exit(1);
}

#[expect(clippy::print_stdout, clippy::print_stderr, reason = "startup messages before logging is set up")]
fn main() {
    util::prevent_root_execution();

    let args = Args::parse();

    // `ed --printenv` Outputs environment variables as JSON to stdout
    if args.printenv {
        shell_env::print_env();
        return;
    }

    if let Some(directory) = args.user_data_dir.as_deref() {
        paths::set_custom_root_dir(directory);
    }

    let file_errors = init_paths();
    if !file_errors.is_empty() {
        files_not_created_on_launch(file_errors);
        return;
    }

    logger::init();

    if stdout_is_a_pty() {
        logger::init_output_stdout();
    } else {
        let result = logger::init_output_file(paths::log_file(), paths::old_log_file());
        if let Err(err) = result {
            eprintln!("Could not open log file: {err}... Defaulting to stdout");
            logger::init_output_stdout();
        }
    }
    install_panic_hook();

    util::increase_open_file_limit().log_err();

    log::info!("========== starting ed version {} ==========", env!("CARGO_PKG_VERSION"));

    let app = build_application().with_assets(Assets);

    let (open_listener, mut open_rx) = OpenListener::new();

    let failed_single_instance_check = if cfg!(debug_assertions) {
        false
    } else {
        {
            use ed::mac_only_instance::IsOnlyInstance;
            use ed::mac_only_instance::ensure_only_instance;
            ensure_only_instance() != IsOnlyInstance::Yes
        }
    };
    if failed_single_instance_check {
        println!("ed is already running");
        return;
    }

    let fs: Arc<dyn Fs> = RealFs::new(app.background_executor());
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
                        fail_to_open_window_async(e, cx);
                    }
                }
            })
            .detach();
        }
    });

    app.run(move |cx| {
        menu::init();
        ed_actions::init();

        initialize_keymap(cx);

        <dyn Fs>::set_global(Arc::clone(&fs), cx);

        OpenListener::set_global(cx, open_listener.clone());

        let languages = Arc::new(LanguageRegistry::new(cx.background_executor().clone()));

        languages::init(Arc::clone(&languages), Arc::clone(&fs), cx);

        ed::init(cx);

        let app_state = Arc::new(AppState { languages, fs: Arc::clone(&fs), build_window_options });
        AppState::set_global(Arc::clone(&app_state), cx);

        command_palette::init(cx);

        recent_projects::init(cx);

        editor::init(cx);
        image_viewer::init(cx);
        diagnostics::init(cx);

        workspace::init(Arc::clone(&app_state), cx);

        go_to_line::init(cx);
        file_finder::init(cx);
        tab_switcher::init(cx);
        project_symbols::init(cx);
        project_panel::init(cx);

        search::init(cx);
        lsp_locations::init(cx);
        cx.set_global(workspace::PaneSearchBarCallbacks {
            setup_search_bar: |toolbar, window, cx| {
                let search_bar = cx.new(|cx| search::BufferSearchBar::new(window, cx));
                toolbar.update(cx, |toolbar, cx| {
                    toolbar.add_item(search_bar, window, cx);
                });
            },
            wrap_div_with_search_actions: buffer_search::register_pane_search_actions,
        });
        terminal_view::init(cx);
        encoding_selector::init(cx);
        language_selector::init(cx);
        lsp_command_selector::init(cx);
        language_tools::init(cx);

        title_bar::init(cx);
        git_ui::init(cx);
        markdown_preview::init(cx);
        svg_preview::init(cx);

        app_state.languages.set_theme(cx.theme());

        initialize_workspace(Arc::clone(&app_state), cx);
        control_server::init(cx);

        cx.activate(true);

        let urls: Vec<_> = args.paths_or_urls.iter().map(|arg| parse_url_arg(arg, cx)).collect();

        // Check if any diff paths are directories to determine diff_all mode
        let (diff_pairs, _) = args.diff.as_chunks::<2>();
        let diff_all_mode = diff_pairs.iter().any(|[old, new]| Path::new(old).is_dir() || Path::new(new).is_dir());
        let diff_paths = diff_pairs.to_vec();

        if !urls.is_empty() || !diff_paths.is_empty() {
            open_listener.open(RawOpenRequest { urls, diff_paths, diff_all: diff_all_mode });
        }

        let restore_task = match open_rx.try_recv().ok().map(OpenRequest::parse) {
            Some(request) => {
                handle_open_request(request, Arc::clone(&app_state), cx);
                Task::ready(())
            }
            None => cx.spawn({
                let app_state = Arc::clone(&app_state);
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx);
                    }
                }
            }),
        };

        let (first_window_tx, first_window_rx) = oneshot::channel::<()>();
        let first_window_tx = Rc::new(RefCell::new(Some(first_window_tx)));
        let _first_window_subscription = cx.observe_new::<Workspace>(move |_, _, _| {
            if let Some(tx) = first_window_tx.borrow_mut().take() {
                tx.send(()).ok();
            }
        });

        let restore_finished = cx.background_spawn(restore_task).shared();

        let app_state = app_state;

        cx.spawn(async move |cx| {
            let _first_window_subscription = _first_window_subscription;
            let first_window_placed = first_window_rx.shared();
            while let Some(urls) = open_rx.next().await {
                // On a macOS cold launch, `ed <path>` arrives here after startup already
                // began restoring the session, so wait for a restored window to exist before
                // matching. Otherwise this open sees no windows and spawns a redundant one (#61346).
                futures::select_biased! {
                    () = restore_finished.clone() => {}
                    _ = first_window_placed.clone() => {}
                }
                cx.update(|cx| {
                    handle_open_request(OpenRequest::parse(urls), Arc::clone(&app_state), cx);
                });
            }
        })
        .detach();
    });
}

fn handle_open_request(request: OpenRequest, app_state: Arc<AppState>, cx: &App) {
    if request.open_paths.is_empty() && request.diff_paths.is_empty() {
        return;
    }
    let task = cx.spawn(async move |cx| {
        let paths_with_position = derive_paths_with_position(app_state.fs.as_ref(), request.open_paths).await;
        let (_window, results) = open_paths_with_positions(
            &paths_with_position,
            &request.diff_paths,
            request.diff_all,
            app_state,
            workspace::OpenOptions::default(),
            cx,
        )
        .await?;
        for result in results.into_iter().flatten() {
            if let Err(err) = result {
                log::error!("Error opening path: {err:#}");
            }
        }
        anyhow::Ok(())
    });
    cx.spawn(async move |cx| {
        if let Err(err) = task.await {
            fail_to_open_window_async(err, cx);
        }
    })
    .detach();
}

pub(crate) async fn restore_or_create_workspace(app_state: Arc<AppState>, cx: &mut AsyncApp) -> Result<()> {
    if let Some(session_workspaces) = restorable_workspaces(cx, &app_state).await {
        let mut error_count = 0;
        for session_workspace in session_workspaces {
            let result = restore_workspace(session_workspace, Arc::clone(&app_state), cx).await.map(|_| ());

            if let Err(error) = result {
                log::error!("Failed to restore workspace: {error:#}");
                error_count += 1;
            }
        }

        if error_count > 0 {
            let message = if error_count == 1 {
                "Failed to restore 1 workspace. Check logs for details.".to_owned()
            } else {
                format!("Failed to restore {error_count} workspaces. Check logs for details.")
            };

            // Try to find an active workspace to show the toast
            let toast_shown = cx.update(|cx| {
                if let Some(window) = cx.active_window()
                    && let Some(workspace) = window.downcast::<Workspace>()
                {
                    workspace
                        .update(cx, |workspace, _, cx| {
                            workspace.show_toast(Toast::new(NotificationId::unique::<()>(), message.clone()), cx);
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
                        Arc::clone(&app_state),
                        cx,
                        |workspace, _window, cx| {
                            workspace.show_toast(Toast::new(NotificationId::unique::<()>(), message), cx);
                        },
                    )
                })
                .await?;
            }
        }
    } else {
        cx.update(|cx| {
            workspace::open_new(workspace::OpenOptions::default(), app_state, cx, |workspace, window, cx| {
                Editor::new_file(workspace, &workspace::NewFile, window, cx);
            })
        })
        .await?;
    }

    Ok(())
}

async fn restorable_workspaces(cx: &AsyncApp, app_state: &Arc<AppState>) -> Option<Vec<SessionWorkspace>> {
    let session = cx.update(|cx| workspace::last_session_workspaces(cx));
    if session.is_empty() {
        workspace::last_opened_workspace_location(app_state.fs.as_ref(), cx).await.map(|workspace| vec![workspace])
    } else {
        Some(session)
    }
}

/// Logs panics with a backtrace, then exits so a panic on any thread takes the
/// whole process down (and the macOS crash dialog never shows).
fn install_panic_hook() {
    let old_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let backtrace = Backtrace::force_capture();
        let message = info.payload_as_str().unwrap_or("Box<Any>");
        let location = info.location().map_or_else(|| "<unknown>".to_owned(), ToString::to_string);
        let thread = thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        log::error!("thread '{thread_name}' panicked at {location}:\n{message}\n{backtrace}");
        logger::flush();
        old_hook(info);
        process::exit(1);
    }));
}

fn init_paths() -> FxHashMap<io::ErrorKind, Vec<&'static Path>> {
    [paths::data_dir(), paths::logs_dir(), paths::temp_dir()].into_iter().fold(
        FxHashMap::default(),
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
#[command(name = "ed", disable_version_flag = true, max_term_width = 100)]
struct Args {
    /// A sequence of space-separated paths or urls that you want to open.
    ///
    /// Use `path:line:row` syntax to open a file at a specific location.
    /// Non-existing paths and directories will ignore `:line:row` suffix.
    ///
    /// URLs can use the `file://` scheme.
    paths_or_urls: Vec<String>,

    /// Pairs of file paths to diff. Can be specified multiple times.
    /// When directories are provided, recurses into them and shows all changed files in a single multi-diff view.
    #[arg(long, action = clap::ArgAction::Append, num_args = 2, value_names = ["OLD_PATH", "NEW_PATH"])]
    diff: Vec<String>,

    /// Sets one root directory for everything this instance writes.
    ///
    /// Data, state (lock, logs) and cache go to `<DIR>/data`, `<DIR>/state`
    /// and `<DIR>/cache` instead of the XDG bases (`~/.local/share/ed`,
    /// `~/.local/state/ed`, `~/.cache/ed`).
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    user_data_dir: Option<String>,

    /// Output current environment variables as JSON to stdout.
    #[arg(long, hide = true)]
    printenv: bool,
}

fn parse_url_arg(arg: &str, _cx: &App) -> String {
    match std::fs::canonicalize(Path::new(&arg)) {
        Ok(path) => format!("file://{}", path.display()),
        Err(_) => {
            if arg.starts_with("file://") {
                arg.into()
            } else {
                format!("file://{arg}")
            }
        }
    }
}
