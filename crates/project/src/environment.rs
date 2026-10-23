use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use collections::HashMap;
use futures::FutureExt;
use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::future::Shared;
use gpui::App;
use gpui::AppContext as _;
use gpui::Context;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::Task;
use gpui::WeakEntity;
use language::Buffer;
use remote::RemoteClient;
use rpc::proto::REMOTE_SERVER_PROJECT_ID;
use rpc::proto::{self};
use task::Shell;
use task::shell_to_proto;
use util::ResultExt;
use worktree::Worktree;

use crate::worktree_store::WorktreeStore;

pub struct ProjectEnvironment {
    cli_environment: Option<HashMap<String, String>>,
    #[allow(clippy::type_complexity)]
    local_environments: HashMap<(Shell, Arc<Path>), Shared<Task<Option<HashMap<String, String>>>>>,
    #[allow(clippy::type_complexity)]
    remote_environments: HashMap<(Shell, Arc<Path>), Shared<Task<Option<HashMap<String, String>>>>>,
    environment_error_messages: VecDeque<String>,
    environment_error_messages_tx: mpsc::UnboundedSender<String>,
    worktree_store: WeakEntity<WorktreeStore>,
    remote_client: Option<WeakEntity<RemoteClient>>,
    is_remote_project: bool,
    _tasks: Vec<Task<()>>,
}

pub enum ProjectEnvironmentEvent {
    ErrorsUpdated,
}

impl EventEmitter<ProjectEnvironmentEvent> for ProjectEnvironment {}

impl ProjectEnvironment {
    pub fn new(
        cli_environment: Option<HashMap<String, String>>,
        worktree_store: WeakEntity<WorktreeStore>,
        remote_client: Option<WeakEntity<RemoteClient>>,
        is_remote_project: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded();
        let task = cx.spawn(async move |this, cx| {
            while let Some(message) = rx.next().await {
                this.update(cx, |this, cx| {
                    this.environment_error_messages.push_back(message);
                    cx.emit(ProjectEnvironmentEvent::ErrorsUpdated);
                })
                .ok();
            }
        });
        Self {
            cli_environment,
            local_environments: Default::default(),
            remote_environments: Default::default(),
            environment_error_messages: Default::default(),
            environment_error_messages_tx: tx,
            worktree_store,
            remote_client,
            is_remote_project,
            _tasks: vec![task],
        }
    }

    /// Returns the inherited CLI environment, if this project was opened from the Zed CLI.
    pub(crate) fn get_cli_environment(&self) -> Option<HashMap<String, String>> {
        if let Some(mut env) = self.cli_environment.clone() {
            set_origin_marker(&mut env, EnvironmentOrigin::Cli);
            Some(env)
        } else {
            None
        }
    }

    pub fn buffer_environment(
        &mut self,
        buffer: &Entity<Buffer>,
        worktree_store: &Entity<WorktreeStore>,
        cx: &mut Context<Self>,
    ) -> Shared<Task<Option<HashMap<String, String>>>> {
        if let Some(cli_environment) = self.get_cli_environment() {
            log::debug!("using project environment variables from CLI");
            return Task::ready(Some(cli_environment)).shared();
        }

        let Some(worktree) = buffer
            .read(cx)
            .file()
            .map(|f| f.worktree_id(cx))
            .and_then(|worktree_id| worktree_store.read(cx).worktree_for_id(worktree_id, cx))
        else {
            return Task::ready(None).shared();
        };
        self.worktree_environment(worktree, cx)
    }

    pub fn worktree_environment(
        &mut self,
        worktree: Entity<Worktree>,
        cx: &mut App,
    ) -> Shared<Task<Option<HashMap<String, String>>>> {
        if let Some(cli_environment) = self.get_cli_environment() {
            log::debug!("using project environment variables from CLI");
            return Task::ready(Some(cli_environment)).shared();
        }

        let worktree = worktree.read(cx);
        let mut abs_path = worktree.abs_path();
        if worktree.is_single_file() {
            let Some(parent) = abs_path.parent() else {
                return Task::ready(None).shared();
            };
            abs_path = parent.into();
        }

        let remote_client = self.remote_client.as_ref().and_then(|it| it.upgrade());
        match remote_client {
            Some(remote_client) => remote_client
                .clone()
                .read(cx)
                .shell()
                .map(|shell| self.remote_directory_environment(&Shell::Program(shell), abs_path, remote_client, cx)),
            None if self.is_remote_project => Some(self.local_directory_environment(&Shell::System, abs_path, cx)),
            None => Some(self.local_directory_environment(&Shell::System, abs_path, cx)),
        }
        .unwrap_or_else(|| Task::ready(None).shared())
    }

    pub fn directory_environment(
        &mut self,
        abs_path: Arc<Path>,
        cx: &mut App,
    ) -> Shared<Task<Option<HashMap<String, String>>>> {
        let remote_client = self.remote_client.as_ref().and_then(|it| it.upgrade());
        match remote_client {
            Some(remote_client) => remote_client
                .clone()
                .read(cx)
                .shell()
                .map(|shell| self.remote_directory_environment(&Shell::Program(shell), abs_path, remote_client, cx)),
            None if self.is_remote_project => Some(self.local_directory_environment(&Shell::System, abs_path, cx)),
            None => self
                .worktree_store
                .read_with(cx, |worktree_store, cx| worktree_store.find_worktree(&abs_path, cx))
                .ok()
                .map(|_| self.local_directory_environment(&Shell::System, abs_path, cx)),
        }
        .unwrap_or_else(|| Task::ready(None).shared())
    }

    /// Returns the project environment using the default worktree path.
    /// This ensures that project-specific environment variables (e.g. from `.envrc`)
    /// are loaded from the project directory rather than the home directory.
    pub fn default_environment(&mut self, cx: &mut App) -> Shared<Task<Option<HashMap<String, String>>>> {
        let abs_path = self
            .worktree_store
            .read_with(cx, |worktree_store, cx| {
                crate::Project::default_visible_worktree_paths(worktree_store, cx).into_iter().next()
            })
            .ok()
            .flatten()
            .map(Arc::<Path>::from)
            .unwrap_or_else(|| paths::home_dir().as_path().into());
        self.local_directory_environment(&Shell::System, abs_path, cx)
    }

    /// Returns the project environment, if possible.
    /// If the project was opened from the CLI, then the inherited CLI environment is returned.
    /// If it wasn't opened from the CLI, and an absolute path is given, then a shell is spawned in
    /// that directory, to get environment variables as if the user has `cd`'d there.
    pub fn local_directory_environment(
        &mut self,
        shell: &Shell,
        abs_path: Arc<Path>,
        cx: &mut App,
    ) -> Shared<Task<Option<HashMap<String, String>>>> {
        if let Some(cli_environment) = self.get_cli_environment() {
            log::debug!("using project environment variables from CLI");
            return Task::ready(Some(cli_environment)).shared();
        }

        self.local_environments
            .entry((shell.clone(), abs_path.clone()))
            .or_insert_with(|| {
                let shell = shell.clone();
                let tx = self.environment_error_messages_tx.clone();
                cx.spawn(async move |cx| {
                    let mut shell_env = match cx
                        .background_spawn(load_directory_shell_environment(shell, abs_path.clone(), tx))
                        .await
                    {
                        Ok(shell_env) => Some(shell_env),
                        Err(e) => {
                            log::error!("Failed to load shell environment for directory {abs_path:?}: {e:#}");
                            None
                        }
                    };

                    if let Some(shell_env) = shell_env.as_mut() {
                        let path = shell_env.get("PATH").map(|path| path.as_str()).unwrap_or_default();
                        log::debug!(
                            "using project environment variables shell launched in {:?}. PATH={:?}",
                            abs_path,
                            path
                        );

                        set_origin_marker(shell_env, EnvironmentOrigin::WorktreeShell);
                    }

                    shell_env
                })
                .shared()
            })
            .clone()
    }

    pub fn remote_directory_environment(
        &mut self,
        shell: &Shell,
        abs_path: Arc<Path>,
        remote_client: Entity<RemoteClient>,
        cx: &mut App,
    ) -> Shared<Task<Option<HashMap<String, String>>>> {
        self.remote_environments
            .entry((shell.clone(), abs_path.clone()))
            .or_insert_with(|| {
                let response = remote_client.read(cx).proto_client().request(proto::GetDirectoryEnvironment {
                    project_id: REMOTE_SERVER_PROJECT_ID,
                    shell: Some(shell_to_proto(shell.clone())),
                    directory: abs_path.to_string_lossy().to_string(),
                });
                cx.background_spawn(async move {
                    let environment = response.await.log_err()?;
                    Some(environment.environment.into_iter().collect())
                })
                .shared()
            })
            .clone()
    }

    pub fn peek_environment_error(&self) -> Option<&String> {
        self.environment_error_messages.front()
    }

    pub fn pop_environment_error(&mut self) -> Option<String> {
        self.environment_error_messages.pop_front()
    }
}

fn set_origin_marker(env: &mut HashMap<String, String>, origin: EnvironmentOrigin) {
    env.insert(ZED_ENVIRONMENT_ORIGIN_MARKER.to_string(), origin.into());
}

const ZED_ENVIRONMENT_ORIGIN_MARKER: &str = "ZED_ENVIRONMENT";

enum EnvironmentOrigin {
    Cli,
    WorktreeShell,
}

impl From<EnvironmentOrigin> for String {
    fn from(val: EnvironmentOrigin) -> Self {
        match val {
            EnvironmentOrigin::Cli => "cli".into(),
            EnvironmentOrigin::WorktreeShell => "worktree-shell".into(),
        }
    }
}

async fn load_directory_shell_environment(
    shell: Shell,
    abs_path: Arc<Path>,
    tx: mpsc::UnboundedSender<String>,
) -> anyhow::Result<HashMap<String, String>> {
    let (shell, args) = shell.program_and_args();
    util::shell_env::capture(shell.clone(), args, abs_path).await.with_context(|| {
        tx.unbounded_send("Failed to load environment variables".into()).ok();
        format!("capturing shell environment with {shell:?}")
    })
}
