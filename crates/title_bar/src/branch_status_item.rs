use git_ui_core::worktree_picker::WorktreePicker;
use gpui::AnyElement;
use gpui::Context;
use gpui::Entity;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Render;
use gpui::Styled;
use gpui::Subscription;
use gpui::WeakEntity;
use gpui::Window;
use project::Project;
use project::git_store::GitStoreEvent;
use project::git_store::Repository;
use project::project_settings::ProjectSettings;
use settings::Settings as _;
use ui::Divider;
use ui::DividerColor;
use ui::PopoverMenu;
use ui::TintColor;
use ui::Tooltip;
use ui::prelude::*;
use workspace::ItemHandle;
use workspace::StatusItemView;
use workspace::Workspace;

use crate::ActiveRepository;
use crate::title_bar_settings::TitleBarSettings;

const MAX_BRANCH_NAME_LENGTH: usize = 40;
const MAX_SHORT_SHA_LENGTH: usize = 8;

/// The left end of the status bar, shown in every mode: the worktree and
/// branch pickers. Shown only for a project in a git repository.
pub struct BranchStatusItem {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    _subscriptions: Vec<Subscription>,
}

impl BranchStatusItem {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let project = workspace.project().clone();
        let git_store = project.read(cx).git_store().clone();
        let mut subscriptions = vec![cx.subscribe(&git_store, |_, _, event, cx| match event {
            GitStoreEvent::ActiveRepositoryChanged(_) | GitStoreEvent::RepositoryUpdated(_, _, true) => {
                cx.notify();
            }
            _ => {}
        })];
        if let Some(workspace_entity) = workspace.weak_handle().upgrade() {
            subscriptions.push(cx.subscribe(&workspace_entity, |_, _, event: &workspace::Event, cx| {
                if matches!(event, workspace::Event::WorktreeCreationChanged) {
                    cx.notify();
                }
            }));
        }
        Self { workspace: workspace.weak_handle(), project, _subscriptions: subscriptions }
    }

    fn render_worktree_and_branch(
        &self,
        repository: Entity<Repository>,
        linked_worktree_name: Option<SharedString>,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let workspace = self.workspace.upgrade()?;

        let (branch_name, icon_info, is_detached_head) = {
            let repo = repository.read(cx);

            let is_detached_head = repo.branch.is_none();

            let branch_name = repo
                .branch
                .as_ref()
                .map(|branch| branch.name())
                .map(|name| util::truncate_and_trailoff(name, MAX_BRANCH_NAME_LENGTH))
                .or_else(|| {
                    repo.head_commit
                        .as_ref()
                        .map(|commit| commit.sha.chars().take(MAX_SHORT_SHA_LENGTH).collect::<String>())
                });

            let status = repo.status_summary();
            let tracked = status.index + status.worktree;
            let icon_info = if status.conflict > 0 {
                (IconName::Warning, Color::VersionControlConflict)
            } else if tracked.modified > 0 {
                (IconName::SquareDot, Color::VersionControlModified)
            } else if tracked.added > 0 || status.untracked > 0 {
                (IconName::SquarePlus, Color::VersionControlAdded)
            } else if tracked.deleted > 0 {
                (IconName::SquareMinus, Color::VersionControlDeleted)
            } else {
                (IconName::GitBranch, Color::Muted)
            };

            (branch_name, icon_info, is_detached_head)
        };

        let settings = TitleBarSettings::get_global(cx);
        let effective_repository = Some(repository);

        let worktree_label: SharedString = linked_worktree_name.unwrap_or_else(|| "main".into());

        let creation = workspace.read(cx).active_worktree_creation();
        let (creation_in_progress, is_switch) = (creation.label.clone(), creation.is_switch);
        let is_creating = creation_in_progress.is_some();

        let display_label: SharedString = if let Some(ref name) = creation_in_progress {
            if is_switch { format!("Loading {}…", name).into() } else { format!("Creating {}…", name).into() }
        } else {
            worktree_label.clone()
        };

        let worktree_button = settings.show_worktree_name.then(|| {
            let project = self.project.clone();
            let workspace_handle = workspace.downgrade();
            PopoverMenu::new("worktree-picker-menu")
                .menu(move |window, cx| {
                    Some(cx.new(|cx| WorktreePicker::new(project.clone(), workspace_handle.clone(), window, cx)))
                })
                .trigger_with_tooltip(
                    Button::new("worktree_picker_trigger", display_label)
                        .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                        .label_size(LabelSize::Small)
                        .color(Color::Muted)
                        .tab_index(0isize)
                        .loading(is_creating)
                        .start_icon(Icon::new(IconName::GitWorktree).size(IconSize::XSmall).color(Color::Muted)),
                    move |_window, cx| {
                        Tooltip::with_meta(
                            "Worktree",
                            Some(&ed_actions::git::Worktree),
                            format!("Currently In Use: {}", worktree_label),
                            cx,
                        )
                    },
                )
                .anchor(gpui::Anchor::BottomLeft)
        });

        let branch_picker = branch_name.and_then(|branch_name| {
            settings.show_branch_name.then(|| {
                let branch_tooltip_label = branch_name.clone();
                let (branch_icon, branch_icon_color) =
                    if settings.show_branch_status_icon { icon_info } else { (IconName::GitBranch, Color::Muted) };

                let trigger = if is_detached_head {
                    Button::new("project_branch_trigger", "Create Branch")
                        .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                        .label_size(LabelSize::Small)
                        .tab_index(0isize)
                        .start_icon(Icon::new(IconName::GitBranchPlus).size(IconSize::XSmall).color(Color::Muted))
                } else {
                    Button::new("project_branch_trigger", branch_name)
                        .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                        .label_size(LabelSize::Small)
                        .color(Color::Muted)
                        .tab_index(0isize)
                        .start_icon(Icon::new(branch_icon).size(IconSize::XSmall).color(branch_icon_color))
                };

                PopoverMenu::new("branch-menu")
                    .menu(move |window, cx| {
                        git_ui_core::build_branch_picker(
                            workspace.downgrade(),
                            effective_repository.clone(),
                            window,
                            cx,
                        )
                    })
                    .trigger_with_tooltip(trigger, move |_window, cx| {
                        let meta = if is_detached_head {
                            format!("Detached HEAD: {}", branch_tooltip_label)
                        } else {
                            format!("Currently Checked Out: {}", branch_tooltip_label)
                        };
                        Tooltip::with_meta("Branch & Stash", Some(&ed_actions::git::Branch), meta, cx)
                    })
                    .anchor(gpui::Anchor::BottomLeft)
            })
        });

        if worktree_button.is_none() && branch_picker.is_none() {
            return None;
        }

        let show_separator = worktree_button.is_some() && branch_picker.is_some();

        Some(
            h_flex()
                .gap_px()
                .children(worktree_button)
                .when(show_separator, |this| {
                    this.child(Label::new("/").size(LabelSize::Small).color(Color::Muted).alpha(0.25))
                })
                .children(branch_picker)
                .into_any_element(),
        )
    }
}

impl Render for BranchStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !ProjectSettings::get_global(cx).git.enabled.status {
            return div().into_any_element();
        }
        let active = ActiveRepository::of(&self.project, cx);
        let Some(branch) = active
            .repository
            .and_then(|repository| self.render_worktree_and_branch(repository, active.linked_worktree_name, cx))
        else {
            return div().into_any_element();
        };

        h_flex().gap_1().child(branch).child(Divider::vertical().color(DividerColor::Border)).into_any_element()
    }
}

impl StatusItemView for BranchStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // The branch doesn't depend on the active item.
    }
}
