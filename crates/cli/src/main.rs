#![allow(
    clippy::disallowed_methods,
    reason = "We are not in an async environment, so std::process::Command is fine"
)]

mod completions;

use crate::completions::Shell;

use anyhow::{Context as _, Result};
use clap::{CommandFactory, Parser};
use cli::{CliRequest, CliResponse, IpcHandshake};
use ipc_channel::ipc::IpcOneShotServer;
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::Arc,
    thread::{self, JoinHandle},
};
use tempfile::{NamedTempFile, TempDir};
use util::paths::PathWithPosition;
use walkdir::WalkDir;

use std::io::IsTerminal;

const URL_PREFIX: [&'static str; 5] = ["zed://", "http://", "https://", "file://", "ssh://"];

struct Detect;

trait InstalledApp {
    fn zed_version_string(&self) -> String;
    fn launch(&self, ipc_url: String, user_data_dir: Option<&str>) -> anyhow::Result<()>;
    fn run_foreground(
        &self,
        ipc_url: String,
        user_data_dir: Option<&str>,
    ) -> io::Result<ExitStatus>;
    fn path(&self) -> PathBuf;
}

#[derive(Parser, Debug)]
#[command(
    name = "zed",
    disable_version_flag = true,
    before_help = "The Zed CLI binary.
This CLI is a separate binary that invokes Zed.

Examples:
    `zed`
          Simply opens Zed
    `zed --foreground`
          Runs in foreground (shows all logs)
    `zed path-to-your-project`
          Open your project in Zed
    `zed -n path-to-file `
          Open file/folder in a new window",
    after_help = "To read from stdin, append '-', e.g. 'ps axf | zed -'"
)]
struct Args {
    /// Wait for all of the given paths to be opened/closed before exiting.
    ///
    /// When opening a directory, waits until the created window is closed.
    #[arg(short, long)]
    wait: bool,
    /// Add files to the currently open workspace
    #[arg(short, long, overrides_with_all = ["new", "reuse", "existing", "classic"])]
    add: bool,
    /// Create a new workspace
    #[arg(short, long, overrides_with_all = ["add", "reuse", "existing", "classic"])]
    new: bool,
    /// Reuse an existing window, replacing its workspace
    #[arg(short, long, overrides_with_all = ["add", "new", "existing", "classic"], hide = true)]
    reuse: bool,
    /// Open in existing Zed window
    #[arg(short = 'e', long = "existing", overrides_with_all = ["add", "new", "reuse", "classic"])]
    existing: bool,
    /// Use the classic open behavior: new window for directories, reuse for files
    #[arg(long, hide = true, overrides_with_all = ["add", "new", "reuse", "existing"])]
    classic: bool,
    /// Sets a custom directory for all user data (e.g., database, extensions, logs).
    /// This overrides the default platform-specific data directory location:
    #[doc = "`~/Library/Application Support/Zed`."]
    #[arg(long, value_name = "DIR", value_hint = clap::ValueHint::DirPath)]
    user_data_dir: Option<String>,
    /// The paths to open in Zed (space-separated).
    ///
    /// Use `path:line:column` syntax to open a file at the given line and column.
    #[arg(trailing_var_arg = true, value_hint = clap::ValueHint::AnyPath)]
    paths_with_position: Vec<String>,
    /// Print Zed's version and the app path.
    #[arg(short, long)]
    version: bool,
    /// Run zed in the foreground (useful for debugging)
    #[arg(long)]
    foreground: bool,
    /// Custom path to Zed.app or the zed binary
    #[arg(long)]
    zed: Option<PathBuf>,
    /// Run zed in dev-server mode
    #[arg(long)]
    dev_server_token: Option<String>,

    /// Not supported in Zed CLI, only supported on Zed binary
    /// Will attempt to give the correct command to run
    #[arg(long)]
    system_specs: bool,
    /// Pairs of file paths to diff. Can be specified multiple times.
    /// When directories are provided, recurses into them and shows all changed files in a single multi-diff view.
    #[arg(long, action = clap::ArgAction::Append, num_args = 2, value_names = ["OLD_PATH", "NEW_PATH"], value_hint = clap::ValueHint::AnyPath)]
    diff: Vec<String>,
    /// Generate shell completions for Zed
    #[arg(long, value_names = ["SHELL"])]
    completions: Option<Shell>,
    /// Used for SSH/Git password authentication, to remove the need for netcat as a dependency,
    /// by having Zed act like netcat communicating over a Unix socket.
    #[arg(long, hide = true)]
    askpass: Option<String>,
}

/// Parses a path containing a position (e.g. `path:line:column`)
/// and returns its canonicalized string representation.
///
/// If a part of path doesn't exist, it will canonicalize the
/// existing part and append the non-existing part.
///
/// This method must return an absolute path, as many zed
/// crates assume absolute paths.
fn parse_path_with_position(argument_str: &str) -> anyhow::Result<String> {
    match Path::new(argument_str).canonicalize() {
        Ok(existing_path) => Ok(PathWithPosition::from_path(existing_path)),
        Err(_) => PathWithPosition::parse_str(argument_str).map_path(|mut path| {
            let curdir = env::current_dir().context("retrieving current directory")?;
            let mut children = Vec::new();
            let root;
            loop {
                // canonicalize handles './', and '/'.
                if let Ok(canonicalized) = fs::canonicalize(&path) {
                    root = canonicalized;
                    break;
                }
                // The comparison to `curdir` is just a shortcut
                // since we know it is canonical. The other one
                // is if `argument_str` is a string that starts
                // with a name (e.g. "foo/bar").
                if path == curdir || path == Path::new("") {
                    root = curdir;
                    break;
                }
                children.push(
                    path.file_name()
                        .with_context(|| format!("parsing as path with position {argument_str}"))?
                        .to_owned(),
                );
                if !path.pop() {
                    unreachable!("parsing as path with position {argument_str}");
                }
            }
            Ok(children.iter().rev().fold(root, |mut path, child| {
                path.push(child);
                path
            }))
        }),
    }
    .map(|path_with_pos| path_with_pos.to_string(&|path| path.to_string_lossy().into_owned()))
}

/// Returns whether a `--diff` argument refers to an existing path, allowing a
/// trailing `:line:column` suffix (parsed later by the Zed side, matching how
/// regular `zed path:line:column` arguments are handled).
fn diff_path_exists(diff_path: &str) -> bool {
    Path::new(diff_path).exists() || PathWithPosition::parse_str(diff_path).path.exists()
}

fn expand_directory_diff_pairs(
    diff_pairs: Vec<[String; 2]>,
) -> anyhow::Result<(Vec<[String; 2]>, Vec<TempDir>)> {
    let mut expanded = Vec::new();
    let mut temp_dirs = Vec::new();

    for pair in diff_pairs {
        let left = PathBuf::from(&pair[0]);
        let right = PathBuf::from(&pair[1]);

        if left.is_dir() && right.is_dir() {
            let (mut pairs, temp_dir) = expand_directory_pair(&left, &right)?;
            expanded.append(&mut pairs);
            if let Some(temp_dir) = temp_dir {
                temp_dirs.push(temp_dir);
            }
        } else {
            expanded.push(pair);
        }
    }

    Ok((expanded, temp_dirs))
}

fn expand_directory_pair(
    left: &Path,
    right: &Path,
) -> anyhow::Result<(Vec<[String; 2]>, Option<TempDir>)> {
    let left_files = collect_files(left)?;
    let right_files = collect_files(right)?;

    let mut rel_paths = BTreeSet::new();
    rel_paths.extend(left_files.keys().cloned());
    rel_paths.extend(right_files.keys().cloned());

    let mut temp_dir = TempDir::new()?;
    let mut temp_dir_used = false;
    let mut pairs = Vec::new();

    for rel in rel_paths {
        match (left_files.get(&rel), right_files.get(&rel)) {
            (Some(left_path), Some(right_path)) => {
                pairs.push([
                    left_path.to_string_lossy().into_owned(),
                    right_path.to_string_lossy().into_owned(),
                ]);
            }
            (Some(left_path), None) => {
                let stub = create_empty_stub(&mut temp_dir, &rel)?;
                temp_dir_used = true;
                pairs.push([
                    left_path.to_string_lossy().into_owned(),
                    stub.to_string_lossy().into_owned(),
                ]);
            }
            (None, Some(right_path)) => {
                let stub = create_empty_stub(&mut temp_dir, &rel)?;
                temp_dir_used = true;
                pairs.push([
                    stub.to_string_lossy().into_owned(),
                    right_path.to_string_lossy().into_owned(),
                ]);
            }
            (None, None) => {}
        }
    }

    let temp_dir = if temp_dir_used { Some(temp_dir) } else { None };
    Ok((pairs, temp_dir))
}

fn collect_files(root: &Path) -> anyhow::Result<BTreeMap<PathBuf, PathBuf>> {
    let mut files = BTreeMap::new();

    for entry in WalkDir::new(root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .context("stripping directory prefix")?
                .to_path_buf();
            files.insert(rel, entry.into_path());
        }
    }

    Ok(files)
}

fn create_empty_stub(temp_dir: &mut TempDir, rel: &Path) -> anyhow::Result<PathBuf> {
    let stub_path = temp_dir.path().join(rel);
    if let Some(parent) = stub_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::File::create(&stub_path)?;
    Ok(stub_path)
}

fn parse_path_in_wsl(source: &str, wsl: &str) -> Result<String> {
    let mut source = PathWithPosition::parse_str(source);

    let (user, distro_name) = if let Some((user, distro)) = wsl.split_once('@') {
        if user.is_empty() {
            anyhow::bail!("user is empty in wsl argument");
        }
        (Some(user), distro)
    } else {
        (None, wsl)
    };

    let mut args = vec!["--distribution", distro_name];
    if let Some(user) = user {
        args.push("--user");
        args.push(user);
    }

    let command = [
        OsStr::new("realpath"),
        OsStr::new("-s"),
        source.path.as_ref(),
    ];

    let output = util::command::new_std_command("wsl.exe")
        .args(&args)
        .arg("--exec")
        .args(&command)
        .output()?;
    let result = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        let fallback = util::command::new_std_command("wsl.exe")
            .args(&args)
            .arg("--")
            .args(&command)
            .output()?;
        String::from_utf8_lossy(&fallback.stdout).to_string()
    };

    source.path = Path::new(result.trim()).to_owned();

    Ok(source.to_string(&|path| path.to_string_lossy().into_owned()))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    util::prevent_root_execution();

    // Exit flatpak sandbox if needed

    // Must happen before clap — SSH invokes cli.exe directly as SSH_ASKPASS
    // and passes the socket path via env var to avoid argument parsing.
    if let Ok(socket) = std::env::var("ZED_ASKPASS_SOCKET") {
        askpass::main_from_args(&socket, std::env::args().skip(1));
        return Ok(());
    }

    let args = Args::parse();

    // `zed --askpass` Makes zed operate in nc/netcat mode for use with askpass
    if let Some(socket) = &args.askpass {
        askpass::main(socket);
        return Ok(());
    }

    // Set custom data directory before any path operations
    let user_data_dir = args.user_data_dir.clone();
    if let Some(dir) = &user_data_dir {
        paths::set_custom_data_dir(dir);
    }

    let app = Detect::detect(args.zed.as_deref()).context("Bundle detection")?;

    if let Some(shell) = &args.completions {
        let file_path = std::env::current_exe()?;
        let file_name = file_path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("--completions expects a UTF-8 name for cli bin")
            .map_err(anyhow::Error::msg)?;
        let mut cmd = Args::command();
        cmd.set_bin_name(file_name);
        cmd.build();
        crate::completions::main(&cmd, shell);
        return Ok(());
    }

    if args.version {
        println!("{}", app.zed_version_string());
        return Ok(());
    }

    if args.system_specs {
        let path = app.path();
        let msg = [
            "The `--system-specs` argument is not supported in the Zed CLI, only on Zed binary.",
            "To retrieve the system specs on the command line, run the following command:",
            &format!("{} --system-specs", path.display()),
        ];
        anyhow::bail!(msg.join("\n"));
    }

    let (server, server_name) =
        IpcOneShotServer::<IpcHandshake>::new().context("Handshake before Zed spawn")?;
    let url = format!("zed-cli://{server_name}");

    let open_behavior = if args.new {
        cli::OpenBehavior::AlwaysNew
    } else if args.add {
        cli::OpenBehavior::Add
    } else if args.existing {
        cli::OpenBehavior::ExistingWindow
    } else if args.classic {
        cli::OpenBehavior::Classic
    } else if args.reuse {
        cli::OpenBehavior::Reuse
    } else {
        cli::OpenBehavior::Default
    };

    let env = {
        {
            use collections::HashMap;

            Some(std::env::vars().collect::<HashMap<_, _>>())
        }
    };

    let exit_status = Arc::new(Mutex::new(None));
    let mut paths = vec![];
    let mut urls = vec![];
    let mut diff_paths = vec![];
    let mut stdin_tmp_file: Option<fs::File> = None;
    let mut anonymous_fd_tmp_files = vec![];

    // Check if any diff paths are directories to determine diff_all mode
    let diff_all_mode = args
        .diff
        .chunks(2)
        .any(|pair| Path::new(&pair[0]).is_dir() || Path::new(&pair[1]).is_dir());

    for path in args.diff.chunks(2) {
        let left = parse_path_with_position(&path[0])?;
        let right = parse_path_with_position(&path[1])?;
        for diff_path in [&left, &right] {
            anyhow::ensure!(
                diff_path_exists(diff_path),
                "--diff path does not exist: {diff_path}"
            );
        }
        diff_paths.push([left, right]);
    }

    let (expanded_diff_paths, temp_dirs) = expand_directory_diff_pairs(diff_paths)?;
    diff_paths = expanded_diff_paths;
    // Prevent automatic cleanup of temp directories containing empty stub files
    // for directory diffs. The CLI process may exit before Zed has read these
    // files (e.g., when RPC-ing into an already-running instance). The files
    // live in the OS temp directory and will be cleaned up on reboot.
    for temp_dir in temp_dirs {
        let _ = temp_dir.keep();
    }

    let wsl = None;

    for path in args.paths_with_position.iter() {
        if URL_PREFIX.iter().any(|&prefix| path.starts_with(prefix)) {
            urls.push(path.to_string());
        } else if path == "-" && args.paths_with_position.len() == 1 {
            let file = NamedTempFile::new()?;
            paths.push(file.path().to_string_lossy().into_owned());
            let (file, _) = file.keep()?;
            stdin_tmp_file = Some(file);
        } else if let Some(file) = anonymous_fd(path) {
            let tmp_file = NamedTempFile::new()?;
            paths.push(tmp_file.path().to_string_lossy().into_owned());
            let (tmp_file, _) = tmp_file.keep()?;
            anonymous_fd_tmp_files.push((file, tmp_file));
        } else if let Some(wsl) = wsl {
            urls.push(format!("file://{}", parse_path_in_wsl(path, wsl)?));
        } else {
            paths.push(parse_path_with_position(path)?);
        }
    }

    anyhow::ensure!(
        args.dev_server_token.is_none(),
        "Dev servers were removed in v0.157.x please upgrade to SSH remoting: https://zed.dev/docs/remote-development"
    );

    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .stack_size(10 * 1024 * 1024)
        .thread_name(|ix| format!("RayonWorker{}", ix))
        .build_global()
        .unwrap();

    let sender: JoinHandle<anyhow::Result<()>> = thread::Builder::new()
        .name("CliReceiver".to_string())
        .spawn({
            let exit_status = exit_status.clone();
            let user_data_dir_for_thread = user_data_dir.clone();
            move || {
                let (_, handshake) = server.accept().context("Handshake after Zed spawn")?;
                let (tx, rx) = (handshake.requests, handshake.responses);

                let wsl = None;

                let open_request = CliRequest::Open {
                    paths,
                    urls,
                    diff_paths,
                    diff_all: diff_all_mode,
                    wsl,
                    wait: args.wait,
                    open_behavior,
                    env,
                    user_data_dir: user_data_dir_for_thread,
                    cwd: env::current_dir().ok(),
                };

                tx.send(open_request)?;

                while let Ok(response) = rx.recv() {
                    match response {
                        CliResponse::Ping => {}
                        CliResponse::Stdout { message } => println!("{message}"),
                        CliResponse::Stderr { message } => eprintln!("{message}"),
                        CliResponse::Exit { status } => {
                            exit_status.lock().replace(status);
                            return Ok(());
                        }
                        CliResponse::PromptOpenBehavior => {
                            let behavior = prompt_open_behavior()
                                .unwrap_or(cli::CliBehaviorSetting::ExistingWindow);
                            tx.send(CliRequest::SetOpenBehavior { behavior })?;
                        }
                    }
                }

                Ok(())
            }
        })
        .unwrap();

    let stdin_pipe_handle: Option<JoinHandle<anyhow::Result<()>>> =
        stdin_tmp_file.map(|mut tmp_file| {
            thread::Builder::new()
                .name("CliStdin".to_string())
                .spawn(move || {
                    let mut stdin = std::io::stdin().lock();
                    if !io::IsTerminal::is_terminal(&stdin) {
                        io::copy(&mut stdin, &mut tmp_file)?;
                    }
                    Ok(())
                })
                .unwrap()
        });

    let anonymous_fd_pipe_handles: Vec<_> = anonymous_fd_tmp_files
        .into_iter()
        .map(|(mut file, mut tmp_file)| {
            thread::Builder::new()
                .name("CliAnonymousFd".to_string())
                .spawn(move || io::copy(&mut file, &mut tmp_file))
                .unwrap()
        })
        .collect();

    if args.foreground {
        app.run_foreground(url, user_data_dir.as_deref())?;
    } else {
        app.launch(url, user_data_dir.as_deref())?;
        sender.join().unwrap()?;
        if let Some(handle) = stdin_pipe_handle {
            handle.join().unwrap()?;
        }
        for handle in anonymous_fd_pipe_handles {
            handle.join().unwrap()?;
        }
    }

    if let Some(exit_status) = exit_status.lock().take() {
        std::process::exit(exit_status);
    }
    Ok(())
}

fn anonymous_fd(path: &str) -> Option<fs::File> {
    {
        use std::os::{
            fd::{self, FromRawFd},
            unix::fs::FileTypeExt,
        };

        let fd_str = path.strip_prefix("/dev/fd/")?;

        let metadata = fs::metadata(path).ok()?;
        let file_type = metadata.file_type();
        if !file_type.is_fifo() && !file_type.is_socket() {
            return None;
        }
        let fd: fd::RawFd = fd_str.parse().ok()?;
        let file = unsafe { fs::File::from_raw_fd(fd) };
        Some(file)
    }
}

/// Shows an interactive prompt asking the user to choose the default open
/// behavior for `zed <path>`. Returns `None` if the prompt cannot be shown
/// (e.g. stdin is not a terminal) or the user cancels.
fn prompt_open_behavior() -> Option<cli::CliBehaviorSetting> {
    if !std::io::stdin().is_terminal() {
        return None;
    }

    let blue = console::Style::new().blue();
    let items = [
        format!(
            "Add to existing Zed window ({})",
            blue.apply_to("zed --existing")
        ),
        format!("Open a new window ({})", blue.apply_to("zed --classic")),
    ];

    let prompt = format!(
        "Configure default behavior for {}\n{}",
        blue.apply_to("zed <path>"),
        console::style("You can change this later in Zed settings"),
    );

    let selection = dialoguer::Select::new()
        .with_prompt(&prompt)
        .items(&items)
        .default(0)
        .interact()
        .ok()?;

    Some(if selection == 0 {
        cli::CliBehaviorSetting::ExistingWindow
    } else {
        cli::CliBehaviorSetting::NewWindow
    })
}

mod mac_os {
    use anyhow::{Context as _, Result};
    use core_foundation::{
        array::{CFArray, CFIndex},
        base::TCFType as _,
        string::kCFStringEncodingUTF8,
        url::{CFURL, CFURLCreateWithBytes},
    };
    use core_services::{
        LSLaunchURLSpec, LSOpenFromURLSpec, kLSLaunchDefaults, kLSLaunchDontSwitch,
    };
    use serde::Deserialize;
    use std::{
        ffi::OsStr,
        fs, io,
        path::{Path, PathBuf},
        process::ExitStatus,
        ptr,
    };

    use cli::FORCE_CLI_MODE_ENV_VAR_NAME;

    use crate::{Detect, InstalledApp};

    #[derive(Debug, Deserialize)]
    struct InfoPlist {
        #[serde(rename = "CFBundleShortVersionString")]
        bundle_short_version_string: String,
    }

    enum Bundle {
        App {
            app_bundle: PathBuf,
            plist: InfoPlist,
        },
        LocalPath {
            executable: PathBuf,
        },
    }

    fn locate_bundle() -> Result<PathBuf> {
        let cli_path = std::env::current_exe()?.canonicalize()?;
        let mut app_path = cli_path.clone();
        while app_path.extension() != Some(OsStr::new("app")) {
            anyhow::ensure!(
                app_path.pop(),
                "cannot find app bundle containing {cli_path:?}"
            );
        }
        Ok(app_path)
    }

    impl Detect {
        pub fn detect(path: Option<&Path>) -> anyhow::Result<impl InstalledApp> {
            let bundle_path = if let Some(bundle_path) = path {
                bundle_path
                    .canonicalize()
                    .with_context(|| format!("Args bundle path {bundle_path:?} canonicalization"))?
            } else {
                locate_bundle().context("bundle autodiscovery")?
            };

            match bundle_path.extension().and_then(|ext| ext.to_str()) {
                Some("app") => {
                    let plist_path = bundle_path.join("Contents/Info.plist");
                    let plist =
                        plist::from_file::<_, InfoPlist>(&plist_path).with_context(|| {
                            format!("Reading *.app bundle plist file at {plist_path:?}")
                        })?;
                    Ok(Bundle::App {
                        app_bundle: bundle_path,
                        plist,
                    })
                }
                _ => Ok(Bundle::LocalPath {
                    executable: bundle_path,
                }),
            }
        }
    }

    impl InstalledApp for Bundle {
        fn zed_version_string(&self) -> String {
            format!("Zed {} – {}", self.version(), self.path().display(),)
        }

        fn launch(&self, url: String, user_data_dir: Option<&str>) -> anyhow::Result<()> {
            match self {
                Self::App { app_bundle, .. } => {
                    let app_path = app_bundle;

                    let status = unsafe {
                        let app_url = CFURL::from_path(app_path, true)
                            .with_context(|| format!("invalid app path {app_path:?}"))?;
                        let url_to_open = CFURL::wrap_under_create_rule(CFURLCreateWithBytes(
                            ptr::null(),
                            url.as_ptr(),
                            url.len() as CFIndex,
                            kCFStringEncodingUTF8,
                            ptr::null(),
                        ));
                        // equivalent to: open zed-cli:... -a /Applications/Zed\ Preview.app
                        let urls_to_open =
                            CFArray::from_copyable(&[url_to_open.as_concrete_TypeRef()]);
                        LSOpenFromURLSpec(
                            &LSLaunchURLSpec {
                                appURL: app_url.as_concrete_TypeRef(),
                                itemURLs: urls_to_open.as_concrete_TypeRef(),
                                passThruParams: ptr::null(),
                                launchFlags: kLSLaunchDefaults | kLSLaunchDontSwitch,
                                asyncRefCon: ptr::null_mut(),
                            },
                            ptr::null_mut(),
                        )
                    };

                    anyhow::ensure!(
                        status == 0,
                        "cannot start app bundle {}",
                        self.zed_version_string()
                    );
                }

                Self::LocalPath { executable, .. } => {
                    let executable_parent = executable
                        .parent()
                        .with_context(|| format!("Executable {executable:?} path has no parent"))?;
                    let subprocess_stdout_file = fs::File::create(
                        executable_parent.join("zed_dev.log"),
                    )
                    .with_context(|| format!("Log file creation in {executable_parent:?}"))?;
                    let subprocess_stdin_file =
                        subprocess_stdout_file.try_clone().with_context(|| {
                            format!("Cloning descriptor for file {subprocess_stdout_file:?}")
                        })?;
                    let mut command = std::process::Command::new(executable);
                    command.env(FORCE_CLI_MODE_ENV_VAR_NAME, "");
                    if let Some(dir) = user_data_dir {
                        command.arg("--user-data-dir").arg(dir);
                    }
                    command
                        .stderr(subprocess_stdout_file)
                        .stdout(subprocess_stdin_file)
                        .arg(url);

                    command
                        .spawn()
                        .with_context(|| format!("Spawning {command:?}"))?;
                }
            }

            Ok(())
        }

        fn run_foreground(
            &self,
            ipc_url: String,
            user_data_dir: Option<&str>,
        ) -> io::Result<ExitStatus> {
            let path = match self {
                Bundle::App { app_bundle, .. } => app_bundle.join("Contents/MacOS/zed"),
                Bundle::LocalPath { executable, .. } => executable.clone(),
            };

            let mut cmd = std::process::Command::new(path);
            cmd.arg(ipc_url);
            if let Some(dir) = user_data_dir {
                cmd.arg("--user-data-dir").arg(dir);
            }
            cmd.status()
        }

        fn path(&self) -> PathBuf {
            match self {
                Bundle::App { app_bundle, .. } => app_bundle.join("Contents/MacOS/zed"),
                Bundle::LocalPath { executable, .. } => executable.clone(),
            }
        }
    }

    impl Bundle {
        fn version(&self) -> String {
            match self {
                Self::App { plist, .. } => plist.bundle_short_version_string.clone(),
                Self::LocalPath { .. } => "<development>".to_string(),
            }
        }

        fn path(&self) -> &Path {
            match self {
                Self::App { app_bundle, .. } => app_bundle,
                Self::LocalPath { executable, .. } => executable,
            }
        }
    }
}
