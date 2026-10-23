use std::process::ExitStatus;

use anyhow::Result;
use gpui::AppContext;
use gpui::AsyncWindowContext;
use gpui::Context;
use gpui::Task;
use gpui::WeakEntity;
use task::ResolvedTask;
use task::SaveStrategy;
use task::SpawnInTerminal;
use ui::Window;
use util::TryFutureExt;

use crate::SaveIntent;
use crate::Toast;
use crate::Workspace;
use crate::notifications::NotificationId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduledTaskResult {
    Success,
    Failure,
    SpawnFailed,
    Cancelled,
}

type TaskCompletionHandler = Box<dyn FnOnce(ScheduledTaskResult, &mut AsyncWindowContext)>;

impl Workspace {
    pub fn schedule_resolved_task(
        self: &mut Workspace,
        resolved_task: ResolvedTask,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        self.schedule_spawn_internal(resolved_task.resolved, None, window, cx);
    }

    /// Spawns an already resolved task again, e.g. to rerun it from its terminal tab.
    pub fn schedule_spawn_in_terminal(
        self: &mut Workspace,
        spawn_in_terminal: SpawnInTerminal,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        self.schedule_spawn_internal(spawn_in_terminal, None, window, cx);
    }

    pub fn schedule_resolved_task_with_completion(
        self: &mut Workspace,
        resolved_task: ResolvedTask,
        on_complete: impl FnOnce(ScheduledTaskResult, &mut AsyncWindowContext) + 'static,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        self.schedule_spawn_internal(resolved_task.resolved, Some(Box::new(on_complete)), window, cx);
    }

    fn schedule_spawn_internal(
        self: &mut Workspace,
        spawn_in_terminal: SpawnInTerminal,
        on_complete: Option<TaskCompletionHandler>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        if self.terminal_provider.is_some() {
            let task = cx.spawn_in(window, async move |workspace, cx| {
                Self::save_for_task(&workspace, spawn_in_terminal.save, cx).await;

                let spawn_task = workspace.update_in(cx, |workspace, window, cx| {
                    workspace
                        .terminal_provider
                        .as_ref()
                        .map(|terminal_provider| terminal_provider.spawn(spawn_in_terminal, window, cx))
                });
                if let Some(spawn_task) = spawn_task.ok().flatten() {
                    let res = cx.background_spawn(spawn_task).await;
                    let result = match res {
                        Some(Ok(status)) => {
                            if status.success() {
                                log::debug!("Task spawn succeeded");
                                ScheduledTaskResult::Success
                            } else {
                                log::debug!("Task spawn failed, code: {:?}", status.code());
                                ScheduledTaskResult::Failure
                            }
                        }
                        Some(Err(e)) => {
                            log::error!("Task spawn failed: {e:#}");
                            _ = workspace.update(cx, |w, cx| {
                                let id = NotificationId::unique::<ResolvedTask>();
                                w.show_toast(Toast::new(id, format!("Task spawn failed: {e}")), cx);
                            });
                            ScheduledTaskResult::SpawnFailed
                        }
                        None => {
                            log::debug!("Task spawn got cancelled");
                            ScheduledTaskResult::Cancelled
                        }
                    };
                    if let Some(on_complete) = on_complete {
                        on_complete(result, cx);
                    }
                } else if let Some(on_complete) = on_complete {
                    on_complete(ScheduledTaskResult::Cancelled, cx);
                }
            });
            self.scheduled_tasks.push(task);
        }
    }

    pub async fn save_for_task(workspace: &WeakEntity<Self>, save_strategy: SaveStrategy, cx: &mut AsyncWindowContext) {
        let save_action = match save_strategy {
            SaveStrategy::All => {
                let save_all = workspace.update_in(cx, |workspace, window, cx| {
                    let task = workspace.save_all_internal(SaveIntent::SaveAll, true, window, cx);
                    cx.background_spawn(async { task.await.map(|_| ()) })
                });
                save_all.ok()
            }
            SaveStrategy::Current => {
                let save_current = workspace
                    .update_in(cx, |workspace, window, cx| workspace.save_active_item(SaveIntent::SaveAll, window, cx));
                save_current.ok()
            }
            SaveStrategy::None => None,
        };
        if let Some(save_action) = save_action {
            save_action.log_err().await;
        }
    }

    pub fn spawn_in_terminal(
        self: &mut Workspace,
        spawn_in_terminal: SpawnInTerminal,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Task<Option<Result<ExitStatus>>> {
        if let Some(terminal_provider) = self.terminal_provider.as_ref() {
            terminal_provider.spawn(spawn_in_terminal, window, cx)
        } else {
            Task::ready(None)
        }
    }
}
