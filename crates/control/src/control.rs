//! Protocol between a running `ed` and the `edctl` automation CLI.
//!
//! The app listens on a Unix socket when started with [`ENABLE_ENV`] or
//! [`SOCKET_ENV`]. Each request and response is one line of JSON.

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Set to any value to listen on [`socket_path_for_pid`].
pub const ENABLE_ENV: &str = "ED_CONTROL";
/// Set to a path to listen on that socket instead.
pub const SOCKET_ENV: &str = "ED_CONTROL_SOCKET";

const SOCKET_PREFIX: &str = "ed-control-";
const SOCKET_SUFFIX: &str = ".sock";

pub fn socket_path_for_pid(pid: u32) -> PathBuf {
    socket_dir().join(format!("{SOCKET_PREFIX}{pid}{SOCKET_SUFFIX}"))
}

/// The socket the app should listen on, if automation is enabled for this process.
pub fn socket_path_from_env() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(SOCKET_ENV) {
        return Some(PathBuf::from(path));
    }
    std::env::var_os(ENABLE_ENV).map(|_| socket_path_for_pid(std::process::id()))
}

/// Sockets of apps started with [`ENABLE_ENV`], including stale ones.
pub fn discover_sockets() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(socket_dir()) else {
        return Vec::new();
    };
    let mut sockets: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(SOCKET_PREFIX) && name.ends_with(SOCKET_SUFFIX))
        })
        .collect();
    sockets.sort();
    sockets
}

fn socket_dir() -> PathBuf {
    // `$TMPDIR` is per user on macOS and short enough for the socket path limit.
    std::env::temp_dir()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    /// Target window; the active window (or the first one) when unset.
    pub window: Option<u64>,
    pub command: Command,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Command {
    /// List windows. Returns `Vec<WindowInfo>`.
    Windows,
    /// Elements and text painted in the last frame. Returns [`Tree`].
    Tree,
    /// Workspace, pane and editor state. Returns [`State`].
    State,
    /// Registered action names, or only those available to the focused element.
    /// Returns `Vec<String>`.
    Actions {
        available: bool,
    },
    /// Dispatch an action to the focused element.
    Action {
        name: String,
        args: Option<Value>,
    },
    /// Space-separated keystrokes in keymap syntax, e.g. `cmd-p enter`.
    Keys {
        keystrokes: String,
    },
    /// Literal text typed one character at a time.
    Type {
        text: String,
    },
    Mouse(MouseCommand),
    /// Scroll by a pixel delta at a window position.
    Scroll {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    },
    /// Write the last frame as a PNG at device resolution.
    Screenshot {
        path: PathBuf,
    },
    /// Wait until the window stops redrawing (or a timeout passes).
    Settle,
    Quit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MouseCommand {
    pub kind: MouseKind,
    pub x: f32,
    pub y: f32,
    pub button: MouseButton,
    pub click_count: usize,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum MouseKind {
    /// Move, press and release.
    Click,
    Down,
    Up,
    /// Move; reports the button as pressed when dragging.
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Ok(Value),
    Error(String),
}

/// A rectangle in logical pixels, relative to the window's content area.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2., self.y + self.height / 2.)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u64,
    pub title: String,
    pub active: bool,
    /// Position on screen and size, in logical pixels.
    pub bounds: Rect,
    pub scale_factor: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tree {
    pub window: u64,
    /// Id of the element that holds focus, if it was painted.
    pub focused: Option<String>,
    /// Elements with an id that take mouse input or focus, in paint order.
    pub elements: Vec<Element>,
    /// Text not painted inside any listed element.
    pub texts: Vec<Text>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Element {
    /// `/`-separated element ids from the root, without view and focus-handle ids.
    pub id: String,
    pub bounds: Rect,
    pub clickable: bool,
    pub focusable: bool,
    pub focused: bool,
    /// Lines of text painted inside this element, but not inside a nested listed element.
    pub texts: Vec<Text>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub bounds: Rect,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    pub window: u64,
    pub workspace: Option<WorkspaceState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub roots: Vec<PathBuf>,
    /// "editor" or "git_history".
    pub mode: String,
    pub panes: Vec<PaneState>,
    /// The Git History mode panes, kept apart from the editor panes.
    pub git_history_panes: Vec<PaneState>,
    /// The active item, when it is an editor.
    pub active_editor: Option<EditorState>,
    pub language_servers: Vec<LanguageServerState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LanguageServerState {
    pub name: String,
    /// Progress the server reports as in flight (e.g. rust-analyzer's indexing).
    pub work: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneState {
    /// The pane the workspace treats as focused (the active one, or a dock's).
    pub focused: bool,
    /// The active pane of its group, where new items open.
    pub active: bool,
    /// Whether keyboard focus is inside the pane.
    pub has_focus: bool,
    pub active_item: Option<usize>,
    pub items: Vec<ItemState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemState {
    pub title: String,
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditorState {
    pub path: Option<PathBuf>,
    pub text: String,
    pub selections: Vec<Selection>,
}

/// Zero-based rows and byte columns.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub start: Position,
    pub end: Position,
    pub reversed: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Position {
    pub row: u32,
    pub column: u32,
}

/// Send one request and wait for its response.
pub fn send(socket: &Path, request: &Request) -> Result<Value> {
    let mut stream =
        UnixStream::connect(socket).with_context(|| format!("failed to connect to {}", socket.display()))?;
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;

    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    anyhow::ensure!(!response.is_empty(), "the app closed the connection without responding");
    match serde_json::from_str::<Response>(&response)? {
        Response::Ok(value) => Ok(value),
        Response::Error(error) => Err(anyhow::anyhow!(error)),
    }
}
