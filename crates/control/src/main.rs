use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use anyhow::Context as _;
use anyhow::Result;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use control::Command;
use control::Element;
use control::Modifiers;
use control::MouseButton;
use control::MouseCommand;
use control::MouseKind;
use control::Rect;
use control::Request;
use control::Text;
use control::Tree;
use control::WindowInfo;
use serde_json::Value;

/// Drive a running `ed` for automated GUI testing.
///
/// Start the app with `edctl launch` (or with `ED_CONTROL=1` in its environment),
/// then inspect it with `tree`, `state` and `screenshot`, and drive it with
/// `action`, `keys`, `type` and `click`. Commands that change the UI return once
/// the window has stopped redrawing.
#[derive(Parser)]
#[command(name = "edctl")]
struct Cli {
    /// Socket of the app to control. Defaults to `$ED_CONTROL_SOCKET`, then the
    /// only running app started with `ED_CONTROL`.
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    /// Window id from `edctl windows`. Defaults to the active window.
    #[arg(long, global = true)]
    window: Option<u64>,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Start `ed` with automation enabled and wait for its first window.
    Launch {
        /// The app binary. Defaults to `ed` next to this binary.
        #[arg(long)]
        app: Option<PathBuf>,
        /// Use a new, empty data directory instead of the user's.
        #[arg(long, conflicts_with = "user_data_dir")]
        fresh: bool,
        #[arg(long)]
        user_data_dir: Option<PathBuf>,
        /// Seconds to wait for the first window.
        #[arg(long, default_value_t = 60.)]
        timeout: f32,
        /// Paths or URLs to open.
        args: Vec<String>,
    },
    /// List windows.
    Windows {
        #[arg(long)]
        json: bool,
    },
    /// Elements and text painted in the last frame.
    Tree {
        /// Only show elements whose id or text contains this.
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Workspace, pane and active editor state as JSON.
    State,
    /// List action names.
    Actions {
        /// Only actions available to the focused element.
        #[arg(long)]
        available: bool,
    },
    /// Dispatch an action to the focused element, e.g. `file_finder::Toggle`.
    Action {
        name: String,
        /// Action arguments as JSON.
        args: Option<String>,
    },
    /// Press keystrokes in keymap syntax, e.g. `cmd-p` or `escape`.
    Keys {
        #[arg(required = true)]
        keystrokes: Vec<String>,
    },
    /// Type literal text.
    Type { text: String },
    /// Click an element, a text, or a position.
    Click {
        #[command(flatten)]
        target: Target,
        #[arg(long, value_enum, default_value_t = MouseButton::Left)]
        button: MouseButton,
        /// 2 for a double click.
        #[arg(long, default_value_t = 1)]
        count: usize,
        #[command(flatten)]
        modifiers: ModifierArgs,
    },
    /// Send a single mouse event, e.g. for dragging.
    Mouse {
        #[arg(value_enum)]
        kind: MouseKind,
        #[command(flatten)]
        target: Target,
        #[arg(long, value_enum, default_value_t = MouseButton::Left)]
        button: MouseButton,
        #[command(flatten)]
        modifiers: ModifierArgs,
    },
    /// Scroll over an element, a text, or a position.
    Scroll {
        #[command(flatten)]
        target: Target,
        #[arg(long, default_value_t = 0., allow_negative_numbers = true)]
        dx: f32,
        /// Positive scrolls content down (towards the end).
        #[arg(long, default_value_t = 0., allow_negative_numbers = true)]
        dy: f32,
    },
    /// Write the window as a PNG at device resolution.
    Screenshot { path: PathBuf },
    /// Wait until an element or text appears (or disappears).
    Wait {
        #[command(flatten)]
        target: Target,
        /// Wait for the element holding focus to match `--id` instead.
        #[arg(long, requires = "id")]
        focused: bool,
        /// Wait until the target is gone.
        #[arg(long)]
        gone: bool,
        /// Seconds.
        #[arg(long, default_value_t = 10.)]
        timeout: f32,
    },
    /// Wait until the window stops redrawing.
    Settle,
    /// Quit the app.
    Quit,
}

#[derive(Args, Clone)]
struct Target {
    #[command(flatten)]
    selector: Selector,
    /// Pick the n-th (0-based) match when several match.
    #[arg(long, conflicts_with = "at")]
    nth: Option<usize>,
}

#[derive(Args, Clone)]
#[group(required = true, multiple = false)]
struct Selector {
    /// Element id, or a `/`-separated suffix of it.
    #[arg(long)]
    id: Option<String>,
    /// Text painted in the window.
    #[arg(long)]
    text: Option<String>,
    /// Window position in logical pixels, `X,Y`.
    #[arg(long, value_parser = parse_point)]
    at: Option<(f32, f32)>,
}

#[derive(Args, Clone, Copy)]
struct ModifierArgs {
    #[arg(long)]
    cmd: bool,
    #[arg(long)]
    shift: bool,
    #[arg(long)]
    alt: bool,
    #[arg(long)]
    ctrl: bool,
}

impl From<ModifierArgs> for Modifiers {
    fn from(args: ModifierArgs) -> Modifiers {
        Modifiers { cmd: args.cmd, shift: args.shift, alt: args.alt, ctrl: args.ctrl }
    }
}

fn parse_point(value: &str) -> Result<(f32, f32), String> {
    let (x, y) = value.split_once(',').ok_or("expected X,Y")?;
    Ok((x.trim().parse().map_err(|_| "invalid X")?, y.trim().parse().map_err(|_| "invalid Y")?))
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    if let CliCommand::Launch { app, fresh, user_data_dir, timeout, args } = cli.command {
        return launch(app, fresh, user_data_dir, timeout, args);
    }

    let client = Client { socket: resolve_socket(cli.socket)?, window: cli.window };
    match cli.command {
        CliCommand::Launch { .. } => unreachable!("handled above"),
        CliCommand::Windows { json } => {
            let windows: Vec<WindowInfo> = client.request(Command::Windows)?;
            if json {
                print_json(&windows)?;
            } else {
                for window in windows {
                    println!(
                        "{}{} {:?} {} scale {}",
                        window.id,
                        if window.active { " (active)" } else { "" },
                        window.title,
                        format_rect(&window.bounds),
                        window.scale_factor
                    );
                }
            }
        }
        CliCommand::Tree { filter, json } => {
            let mut tree: Tree = client.request(Command::Tree)?;
            if let Some(filter) = &filter {
                tree.elements.retain(|element| {
                    element.id.contains(filter.as_str())
                        || element.texts.iter().any(|text| text.text.contains(filter.as_str()))
                });
                tree.texts.retain(|text| text.text.contains(filter.as_str()));
            }
            if json { print_json(&tree)? } else { print_tree(&tree) }
        }
        CliCommand::State => print_json(&client.request::<Value>(Command::State)?)?,
        CliCommand::Actions { available } => {
            let names: Vec<String> = client.request(Command::Actions { available })?;
            for name in names {
                println!("{name}");
            }
        }
        CliCommand::Action { name, args } => {
            let args = args.map(|args| serde_json::from_str(&args)).transpose().context("invalid action arguments")?;
            client.request::<Value>(Command::Action { name, args })?;
        }
        CliCommand::Keys { keystrokes } => {
            client.request::<Value>(Command::Keys { keystrokes: keystrokes.join(" ") })?;
        }
        CliCommand::Type { text } => {
            client.request::<Value>(Command::Type { text })?;
        }
        CliCommand::Click { target, button, count, modifiers } => {
            let (x, y) = client.resolve_point(&target)?;
            let mouse =
                MouseCommand { kind: MouseKind::Click, x, y, button, click_count: count, modifiers: modifiers.into() };
            client.request::<Value>(Command::Mouse(mouse))?;
            println!("clicked {x},{y}");
        }
        CliCommand::Mouse { kind, target, button, modifiers } => {
            let (x, y) = client.resolve_point(&target)?;
            let mouse = MouseCommand { kind, x, y, button, click_count: 1, modifiers: modifiers.into() };
            client.request::<Value>(Command::Mouse(mouse))?;
        }
        CliCommand::Scroll { target, dx, dy } => {
            let (x, y) = client.resolve_point(&target)?;
            client.request::<Value>(Command::Scroll { x, y, dx, dy })?;
        }
        CliCommand::Screenshot { path } => {
            let path = std::path::absolute(&path)?;
            client.request::<Value>(Command::Screenshot { path: path.clone() })?;
            println!("{}", path.display());
        }
        CliCommand::Wait { target, focused, gone, timeout } => client.wait(&target, focused, gone, timeout)?,
        CliCommand::Settle => {
            client.request::<Value>(Command::Settle)?;
        }
        CliCommand::Quit => {
            client.request::<Value>(Command::Quit)?;
        }
    }
    Ok(())
}

struct Client {
    socket: PathBuf,
    window: Option<u64>,
}

impl Client {
    fn request<T: serde::de::DeserializeOwned>(&self, command: Command) -> Result<T> {
        let value = control::send(&self.socket, &Request { window: self.window, command })?;
        Ok(serde_json::from_value(value)?)
    }

    fn resolve_point(&self, target: &Target) -> Result<(f32, f32)> {
        if let Some(point) = target.selector.at {
            return Ok(point);
        }
        let tree: Tree = self.request(Command::Tree)?;
        let matches = find_targets(&tree, target);
        let rect = match (matches.as_slice(), target.nth) {
            ([], _) => anyhow::bail!("nothing matches {}", describe_target(target)),
            (_, Some(nth)) => matches.get(nth).with_context(|| format!("only {} matches", matches.len()))?.rect,
            ([only], None) => only.rect,
            (_, None) => {
                let mut message = format!("{} matches {}; pass --nth:", matches.len(), describe_target(target));
                for (ix, found) in matches.iter().enumerate() {
                    write!(message, "\n  {ix}: {} {}", found.label, format_rect(&found.rect))?;
                }
                anyhow::bail!(message)
            }
        };
        Ok(rect.center())
    }

    fn wait(&self, target: &Target, focused: bool, gone: bool, timeout: f32) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs_f32(timeout);
        loop {
            let tree: Tree = self.request(Command::Tree)?;
            let found = if focused {
                let id = target.selector.id.as_deref().unwrap_or_default();
                tree.focused.as_deref().is_some_and(|focused| id_matches(focused, id))
            } else {
                !find_targets(&tree, target).is_empty()
            };
            if found != gone {
                return Ok(());
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "timed out waiting for {}{}",
                describe_target(target),
                if gone { " to disappear" } else { "" }
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

struct Found {
    label: String,
    rect: Rect,
}

fn find_targets(tree: &Tree, target: &Target) -> Vec<Found> {
    if let Some(id) = &target.selector.id {
        let exact: Vec<&Element> = tree.elements.iter().filter(|element| id_matches(&element.id, id)).collect();
        let elements = if exact.is_empty() {
            tree.elements.iter().filter(|element| element.id.contains(id.as_str())).collect()
        } else {
            exact
        };
        return elements.into_iter().map(|element| Found { label: element.id.clone(), rect: element.bounds }).collect();
    }

    let Some(text) = &target.selector.text else {
        return Vec::new();
    };
    let all_texts = || {
        tree.elements
            .iter()
            .flat_map(|element| element.texts.iter().map(move |text| (element.id.as_str(), text)))
            .chain(tree.texts.iter().map(|text| ("", text)))
    };
    let mut found: Vec<(&str, &Text)> =
        all_texts().filter(|(_, painted)| painted.text.trim() == text.as_str()).collect();
    if found.is_empty() {
        found = all_texts().filter(|(_, painted)| painted.text.contains(text.as_str())).collect();
    }
    found
        .into_iter()
        .map(|(id, painted)| Found { label: format!("{id} {:?}", painted.text), rect: painted.bounds })
        .collect()
}

/// Whether `id` is `pattern` or ends with it on a `/` boundary.
fn id_matches(id: &str, pattern: &str) -> bool {
    id == pattern || id.strip_suffix(pattern).is_some_and(|prefix| prefix.ends_with('/'))
}

fn describe_target(target: &Target) -> String {
    match (&target.selector.id, &target.selector.text) {
        (Some(id), _) => format!("id {id:?}"),
        (_, Some(text)) => format!("text {text:?}"),
        _ => "target".into(),
    }
}

fn format_rect(rect: &Rect) -> String {
    format!("[{},{} {}x{}]", rect.x.round(), rect.y.round(), rect.width.round(), rect.height.round())
}

fn print_tree(tree: &Tree) {
    println!("window {} focused: {}", tree.window, tree.focused.as_deref().unwrap_or("-"));
    for element in &tree.elements {
        let flags = format!(
            "{}{}{}",
            if element.clickable { "c" } else { "-" },
            if element.focusable { "f" } else { "-" },
            if element.focused { "*" } else { "-" }
        );
        let texts: Vec<String> = element.texts.iter().map(|text| format!("{:?}", text.text)).collect();
        println!("{flags} {} {} {}", element.id, format_rect(&element.bounds), texts.join(" "));
    }
    for text in &tree.texts {
        println!("text {} {:?}", format_rect(&text.bounds), text.text);
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn resolve_socket(socket: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(socket) = socket.or_else(|| std::env::var_os(control::SOCKET_ENV).map(PathBuf::from)) {
        return Ok(socket);
    }
    let mut live = Vec::new();
    for socket in control::discover_sockets() {
        match std::os::unix::net::UnixStream::connect(&socket) {
            Ok(_) => live.push(socket),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                std::fs::remove_file(&socket).ok();
            }
            Err(_) => {}
        }
    }
    match live.len() {
        0 => anyhow::bail!("no running app with automation enabled; start one with `edctl launch`"),
        1 => Ok(live.remove(0)),
        _ => {
            let sockets: Vec<String> = live.iter().map(|socket| socket.display().to_string()).collect();
            anyhow::bail!("several apps are running; pass --socket:\n  {}", sockets.join("\n  "))
        }
    }
}

#[expect(clippy::disallowed_methods, reason = "edctl is a blocking CLI with no async runtime")]
fn launch(
    app: Option<PathBuf>,
    fresh: bool,
    user_data_dir: Option<PathBuf>,
    timeout: f32,
    args: Vec<String>,
) -> Result<()> {
    let app = match app {
        Some(app) => app,
        None => std::env::current_exe()?.with_file_name("ed"),
    };
    let user_data_dir = if fresh {
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_nanos();
        let dir = std::env::temp_dir().join(format!("ed-control-data-{nanos}"));
        std::fs::create_dir_all(&dir)?;
        Some(dir)
    } else {
        user_data_dir
    };

    let mut command = std::process::Command::new(&app);
    if let Some(dir) = &user_data_dir {
        command.arg("--user-data-dir").arg(dir);
    }
    command
        .args(&args)
        .env(control::ENABLE_ENV, "1")
        .env_remove(control::SOCKET_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().with_context(|| format!("failed to start {}", app.display()))?;
    let socket = control::socket_path_for_pid(child.id());

    let deadline = Instant::now() + Duration::from_secs_f32(timeout);
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("{} exited with {status} before opening a window", app.display());
        }
        let request = Request { window: None, command: Command::Windows };
        if let Ok(windows) = control::send(&socket, &request)
            && windows.as_array().is_some_and(|windows| !windows.is_empty())
        {
            break;
        }
        anyhow::ensure!(Instant::now() < deadline, "timed out waiting for {} to open a window", app.display());
        std::thread::sleep(Duration::from_millis(100));
    }

    println!("pid {}", child.id());
    println!("socket {}", socket.display());
    if let Some(dir) = user_data_dir {
        println!("user data dir {}", dir.display());
    }
    Ok(())
}
