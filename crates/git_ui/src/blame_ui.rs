use crate::{
    commit_tooltip::CommitTooltip,
    commit_view::{CommitView, GitBlob, build_buffer, worktree_id_for_repo_path},
};
use anyhow::Context as _;
use editor::{BlameRenderer, Editor, GitBlame, MultiBuffer};
use git::{Oid, blame::BlameEntry, commit::ParsedCommitMessage, repository::RepoPath};
use gpui::{
    ClipboardItem, Entity, Hsla, MouseButton, Pixels, Rems, Subscription, TextStyle, WeakEntity,
    prelude::*,
};
use project::git_store::Repository;
use std::sync::Arc;
use ui::{ContextMenu, prelude::*};
use util::paths::PathStyle;
use workspace::{Workspace, notifications::NotifyTaskExt as _};

const GIT_BLAME_MAX_AUTHOR_CHARS_DISPLAYED: usize = 20;
const GIT_BLAME_GUTTER_MARGIN: Rems = rems(0.5);
const GIT_BLAME_GUTTER_GAP: Rems = rems(0.5);

pub struct GitBlameRenderer;

impl BlameRenderer for GitBlameRenderer {
    fn max_author_length(&self) -> usize {
        GIT_BLAME_MAX_AUTHOR_CHARS_DISPLAYED
    }

    fn blame_entry_non_text_width(&self, window: &Window, _cx: &App) -> Pixels {
        GIT_BLAME_GUTTER_MARGIN.to_pixels(window.rem_size())
            + GIT_BLAME_GUTTER_GAP.to_pixels(window.rem_size()) * 2.
    }

    fn render_blame_entry(
        &self,
        style: &TextStyle,
        blame_entry: BlameEntry,
        details: Option<ParsedCommitMessage>,
        tag_names: Vec<SharedString>,
        repository: Entity<Repository>,
        workspace: WeakEntity<Workspace>,
        editor: Entity<Editor>,
        ix: usize,
        sha_color: Hsla,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let relative_timestamp = blame_entry_relative_timestamp(&blame_entry);
        let short_commit_id = blame_entry.sha.display_short();
        let author_name = blame_entry.author.as_deref().unwrap_or("<no name>");
        let name = util::truncate_and_trailoff(author_name, GIT_BLAME_MAX_AUTHOR_CHARS_DISPLAYED);
        let is_highlighted = editor
            .read(cx)
            .blame()
            .is_some_and(|blame| blame.read(cx).highlighted_sha() == Some(blame_entry.sha));

        Some(
            div()
                .mr_2()
                .child(
                    h_flex()
                        .id(("blame", ix))
                        .w_full()
                        .gap_2()
                        .justify_between()
                        .font(style.font())
                        .line_height(style.line_height)
                        .text_color(cx.theme().status().hint)
                        .when(is_highlighted, |this| {
                            this.bg(cx.theme().colors().element_selected)
                        })
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().text_color(sha_color).child(short_commit_id))
                                .child(name),
                        )
                        .child(relative_timestamp)
                        .hover(|style| style.bg(cx.theme().colors().element_hover))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Right, {
                            let blame_entry = blame_entry.clone();
                            let editor = editor.clone();
                            let repository = repository.clone();
                            let workspace = workspace.clone();
                            move |event, window, cx| {
                                cx.stop_propagation();

                                deploy_blame_entry_context_menu(
                                    &blame_entry,
                                    editor.clone(),
                                    repository.clone(),
                                    workspace.clone(),
                                    event.position,
                                    window,
                                    cx,
                                );
                            }
                        })
                        .on_click({
                            let blame_entry = blame_entry.clone();
                            let repository = repository.clone();
                            let workspace = workspace.clone();
                            move |_, window, cx| {
                                CommitView::open(
                                    blame_entry.sha.to_string(),
                                    repository.downgrade(),
                                    workspace.clone(),
                                    None,
                                    None,
                                    window,
                                    cx,
                                )
                            }
                        })
                        .when(!editor.read(cx).has_mouse_context_menu(), |el| {
                            el.hoverable_tooltip(move |_window, cx| {
                                cx.new(|cx| {
                                    CommitTooltip::blame_entry(
                                        &blame_entry,
                                        details.clone(),
                                        tag_names.clone(),
                                        repository.clone(),
                                        workspace.clone(),
                                        cx,
                                    )
                                })
                                .into()
                            })
                        }),
                )
                .into_any(),
        )
    }

    fn open_blame_commit(
        &self,
        blame_entry: BlameEntry,
        repository: Entity<Repository>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) {
        CommitView::open(
            blame_entry.sha.to_string(),
            repository.downgrade(),
            workspace,
            None,
            None,
            window,
            cx,
        )
    }

    fn open_blame_revision(
        &self,
        path: RepoPath,
        revision: Oid,
        repository: Entity<Repository>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) {
        open_buffer_blame_at_revision(repository, workspace, path, revision, window, cx);
    }
}

fn deploy_blame_entry_context_menu(
    blame_entry: &BlameEntry,
    editor: Entity<Editor>,
    repository: Entity<Repository>,
    workspace: WeakEntity<Workspace>,
    position: gpui::Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let highlighted_sha = editor
        .read(cx)
        .blame()
        .and_then(|blame| blame.read(cx).highlighted_sha());
    let context_menu = ContextMenu::build(window, cx, move |menu, _, _| {
        let sha = format!("{}", blame_entry.sha);
        let blame_revision = blame_entry.revision_target(highlighted_sha);
        let blame_previous_revision = blame_entry.previous_revision_target();
        let has_blame_targets = blame_revision.is_some() || blame_previous_revision.is_some();
        menu.on_blur_subscription(Subscription::new(|| {}))
            .entry("Copy Commit SHA", None, move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(sha.clone()));
            })
            .when(has_blame_targets, |this| this.separator())
            .when_some(blame_revision, |this, (revision, path)| {
                let repository = repository.clone();
                let workspace = workspace.clone();
                this.entry("Blame Revision", None, move |window, cx| {
                    open_buffer_blame_at_revision(
                        repository.clone(),
                        workspace.clone(),
                        path.clone(),
                        revision,
                        window,
                        cx,
                    );
                })
            })
            .when_some(blame_previous_revision, |this, (revision, path)| {
                let repository = repository.clone();
                let workspace = workspace.clone();
                this.entry("Blame Previous Revision", None, move |window, cx| {
                    open_buffer_blame_at_revision(
                        repository.clone(),
                        workspace.clone(),
                        path.clone(),
                        revision,
                        window,
                        cx,
                    );
                })
            })
    });

    editor.update(cx, move |editor, cx| {
        editor.deploy_mouse_context_menu(position, context_menu, window, cx);
        cx.notify();
    });
}

fn open_buffer_blame_at_revision(
    repository: Entity<Repository>,
    workspace: WeakEntity<Workspace>,
    path: RepoPath,
    revision: Oid,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, {
            let workspace = workspace.clone();
            async move |cx| {
                let (language_registry, worktree_id) =
                    workspace.read_with(cx, |workspace, cx| {
                        let project = workspace.project().read(cx);
                        (
                            project.languages().clone(),
                            worktree_id_for_repo_path(repository.read(cx), project, &path, cx),
                        )
                    })?;
                let worktree_id = worktree_id.context("project has no worktrees")?;

                let file_name = path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| path.display(PathStyle::local()).to_string());
                let display_name = format!("{file_name} @ {}", revision.display_short());

                let activated_existing = workspace.update_in(cx, |workspace, window, cx| {
                    activate_existing_blame_editor(
                        workspace,
                        &repository,
                        &path,
                        revision,
                        window,
                        cx,
                    )
                })?;
                if activated_existing {
                    return Ok(());
                }

                let (content, blame) = repository
                    .update(cx, |repository, cx| {
                        repository.blame_buffer_at_revision(path.clone(), revision, cx)
                    })
                    .await?;

                let file = Arc::new(GitBlob {
                    path: path.clone(),
                    worktree_id,
                    is_deleted: false,
                    is_binary: false,
                    display_name: display_name.clone(),
                }) as Arc<dyn language::File>;

                let buffer = build_buffer(content, file, &language_registry, cx).await?;

                workspace.update_in(cx, |workspace, window, cx| {
                    if activate_existing_blame_editor(
                        workspace,
                        &repository,
                        &path,
                        revision,
                        window,
                        cx,
                    ) {
                        return;
                    }

                    let project = workspace.project().clone();
                    let multi_buffer = cx.new(|cx| MultiBuffer::singleton(buffer.clone(), cx));
                    let editor = cx.new(|cx| {
                        let mut editor = Editor::for_multibuffer(
                            multi_buffer.clone(),
                            Some(project.clone()),
                            window,
                            cx,
                        );
                        editor.set_read_only(true);
                        editor.set_should_serialize(false, cx);
                        editor
                    });
                    let git_blame = cx.new(|cx| {
                        GitBlame::new_static(
                            multi_buffer,
                            project,
                            repository,
                            [(buffer, blame)],
                            Some(revision),
                            cx,
                        )
                    });
                    editor.update(cx, |editor, cx| editor.set_blame(git_blame, window, cx));
                    workspace.add_item_to_active_pane(Box::new(editor), None, true, window, cx);
                })
            }
        })
        .detach_and_notify_err(workspace, window, cx);
}

fn activate_existing_blame_editor(
    workspace: &mut Workspace,
    repository: &Entity<Repository>,
    path: &RepoPath,
    revision: Oid,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> bool {
    match existing_blame_editor(workspace, repository, path, revision, cx) {
        Some(existing) => {
            workspace.activate_item(&existing, true, true, window, cx);
            true
        }
        None => false,
    }
}

fn existing_blame_editor(
    workspace: &Workspace,
    repository: &Entity<Repository>,
    path: &RepoPath,
    revision: Oid,
    cx: &App,
) -> Option<Entity<Editor>> {
    workspace
        .panes()
        .iter()
        .flat_map(|pane| pane.read(cx).items())
        .find_map(|item| {
            let editor = item.downcast::<Editor>()?;
            let blame = editor.read(cx).blame()?;
            if blame.read(cx).highlighted_sha() != Some(revision) {
                return None;
            }
            let buffer = editor.read(cx).buffer().read(cx).as_singleton()?;
            let buffer_id = buffer.read(cx).remote_id();
            if blame.read(cx).repository(cx, buffer_id).as_ref() != Some(repository) {
                return None;
            }
            let file = buffer.read(cx).file()?;
            let blob = (file.as_ref() as &dyn std::any::Any).downcast_ref::<GitBlob>()?;
            (blob.path == *path).then_some(editor)
        })
}

fn blame_entry_relative_timestamp(blame_entry: &BlameEntry) -> String {
    match blame_entry.author_offset_date_time() {
        Ok(timestamp) => {
            let local_offset =
                time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
            time_format::format_localized_timestamp(
                timestamp,
                time::OffsetDateTime::now_utc(),
                local_offset,
                time_format::TimestampFormat::Relative,
            )
        }
        Err(_) => "Error parsing date".to_string(),
    }
}
