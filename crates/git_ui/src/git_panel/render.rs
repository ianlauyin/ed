use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use ed_actions::editor::MoveDown;
use ed_actions::editor::MoveUp;
use ed_actions::git::ViewStash;
use ed_actions::workspace::CopyPath;
use ed_actions::workspace::CopyRelativePath;
use editor::EditorElement;
use file_icons::FileIcons;
use git::Amend;
use git::DeleteUntrackedFiles;
use git::RestoreFile;
use git::RestoreTrackedFiles;
use git::Signoff;
use git::SkipHooks;
use git::StageAll;
use git::StashAll;
use git::StashPop;
use git::StashStaged;
use git::StashTracked;
use git::ToggleStaged;
use git::UnstageAll;
use git::ViewFile;
use git::commit::ParsedCommitMessage;
use git::repository::RebaseAction;
use git::status::DiffStat;
use git::status::StageStatus;
use gpui::Action as _;
use gpui::Anchor;
use gpui::AnyElement;
use gpui::AppContext as _;
use gpui::ClickEvent;
use gpui::DismissEvent;
use gpui::Empty;
use gpui::Entity;
use gpui::FocusHandle;
use gpui::Focusable as _;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::Pixels;
use gpui::Point;
use gpui::Subscription;
use gpui::anchored;
use gpui::deferred;
use gpui::uniform_list;
use language::File;
use menu;
use project::git_store::GitAccess;
use project::git_store::Repository;
use smallvec::SmallVec;
use time::OffsetDateTime;
use ui::ButtonLike;
use ui::Checkbox;
use ui::Chip;
use ui::ContextMenu;
use ui::ContextMenuEntry;
use ui::DocumentationSide;
use ui::ElevationIndex;
use ui::IndentGuideColors;
use ui::KeyBinding;
use ui::PopoverMenu;
use ui::ProjectEmptyState;
use ui::ScrollAxes;
use ui::Scrollbars;
use ui::SplitButton;
use ui::Tab;
use ui::Tooltip;
use ui::WithScrollbar as _;
use ui::prelude::ActiveTheme as _;
use ui::prelude::App;
use ui::prelude::Button;
use ui::prelude::ButtonCommon as _;
use ui::prelude::ButtonSize;
use ui::prelude::ButtonStyle;
use ui::prelude::Clickable as _;
use ui::prelude::Color;
use ui::prelude::Context;
use ui::prelude::Disableable as _;
use ui::prelude::Div;
use ui::prelude::ElementId;
use ui::prelude::FluentBuilder as _;
use ui::prelude::Icon;
use ui::prelude::IconButton;
use ui::prelude::IconName;
use ui::prelude::IconPosition;
use ui::prelude::IconSize;
use ui::prelude::InteractiveElement as _;
use ui::prelude::IntoElement;
use ui::prelude::Label;
use ui::prelude::LabelCommon as _;
use ui::prelude::LabelSize;
use ui::prelude::ParentElement as _;
use ui::prelude::Rems;
use ui::prelude::Render;
use ui::prelude::SharedString;
use ui::prelude::StatefulInteractiveElement as _;
use ui::prelude::Styled as _;
use ui::prelude::ToggleState;
use ui::prelude::Toggleable as _;
use ui::prelude::Window;
use ui::prelude::div;
use ui::prelude::h_flex;
use ui::prelude::px;
use ui::prelude::relative;
use ui::prelude::v_flex;
use workspace::CARD_RADIUS;
use workspace::dock::Panel as _;

use super::GitHeaderEntry;
use super::GitListEntry;
use super::GitPanel;
use super::GitPanelViewMode;
use super::GitStatusEntry;
use super::GitTreeDirEntry;
use super::GitTreeStatusEntry;
use super::INDENT_GUIDE_LEFT_OFFSET;
use super::LIST_ITEM_HEIGHT;
use super::MAX_PANEL_EDITOR_LINES;
use super::Section;
use super::SelectionTargetKind;
use super::SetGroupByNone;
use super::SetGroupByStaging;
use super::SetGroupByStatus;
use super::SetSortByName;
use super::SetSortByPath;
use super::TREE_INDENT;
use super::ToggleTreeView;
use super::ViewStagedChanges;
use super::ViewUnstagedChanges;
use super::discard::restore_title;
use super::git_commit_editor_style;
use super::git_commit_font_size;
use super::panel_editor_container;
use super::staging::StageIntent;
use crate::commit_tooltip;
use crate::commit_tooltip::CommitTooltip;
use crate::commit_view::CommitView;
use crate::git_panel_view_options::GitPanelGroupBy;
use crate::git_panel_view_options::GitPanelSortBy;
use crate::git_panel_view_options::GitPanelViewOptions;
use crate::git_status_icon;
use crate::project_diff::DeployBranchDiff;
use crate::project_diff::Diff;
use crate::render_remote_button;

fn git_panel_context_menu(
    has_tracked_changes: bool,
    has_staged_tracked_changes: bool,
    has_staged_changes: bool,
    has_unstaged_changes: bool,
    has_new_changes: bool,
    has_stash_items: bool,
    can_stash: bool,
    group_by: GitPanelGroupBy,
    include_copy_paths: bool,
    focus_handle: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ContextMenu> {
    ContextMenu::build(window, cx, |context_menu, _, _| {
        context_menu
            .context(focus_handle.clone())
            .action_disabled_when(!has_unstaged_changes, "Stage All", StageAll.boxed_clone())
            .action_disabled_when(!has_staged_changes, "Unstage All", UnstageAll.boxed_clone())
            .action_disabled_when(!has_staged_tracked_changes, "Restore All Changes", RestoreTrackedFiles.boxed_clone())
            .separator()
            .action_disabled_when(
                !(can_stash && (has_new_changes || has_tracked_changes)),
                "Stash All",
                StashAll.boxed_clone(),
            )
            // Offer the stash variant that matches how the list is currently grouped,
            // so the menu mirrors the sections the user can actually see.
            .when(group_by == GitPanelGroupBy::Status, |context_menu| {
                context_menu.action_disabled_when(
                    !(can_stash && has_tracked_changes),
                    "Stash Tracked",
                    StashTracked.boxed_clone(),
                )
            })
            .when(group_by == GitPanelGroupBy::Staging, |context_menu| {
                context_menu.action_disabled_when(
                    !(can_stash && has_staged_changes),
                    "Stash Staged",
                    StashStaged.boxed_clone(),
                )
            })
            .action_disabled_when(!(can_stash && has_stash_items), "Stash Pop", StashPop.boxed_clone())
            .action("View Stash", ViewStash.boxed_clone())
            .when(include_copy_paths, |context_menu| {
                context_menu
                    .separator()
                    .action("Copy Path", CopyPath.boxed_clone())
                    .action("Copy Relative Path", CopyRelativePath.boxed_clone())
            })
            .separator()
            .action_disabled_when(
                !has_staged_tracked_changes,
                "Discard Tracked Changes",
                RestoreTrackedFiles.boxed_clone(),
            )
            .action_disabled_when(!has_new_changes, "Delete Untracked Files", DeleteUntrackedFiles.boxed_clone())
    })
}

fn git_panel_view_options_menu(focus_handle: FocusHandle, window: &mut Window, cx: &mut App) -> Entity<ContextMenu> {
    let view_options_menu_state = Rc::new(Cell::new(GitPanelViewOptions::get(cx)));

    ContextMenu::build_persistent(window, cx, move |context_menu, _, _| {
        let state = view_options_menu_state.get();

        context_menu
            .context(focus_handle.clone())
            .header("View")
            .item({
                let view_options_menu_state = Rc::clone(&view_options_menu_state);
                ContextMenuEntry::new("List").toggle(IconPosition::End, !state.tree_view).handler(move |window, cx| {
                    if state.tree_view {
                        view_options_menu_state.set(GitPanelViewOptions { tree_view: false, ..state });
                        window.dispatch_action(Box::new(ToggleTreeView), cx);
                    }
                })
            })
            .item({
                let view_options_menu_state = Rc::clone(&view_options_menu_state);
                ContextMenuEntry::new("Tree").toggle(IconPosition::End, state.tree_view).handler(move |window, cx| {
                    if !state.tree_view {
                        view_options_menu_state.set(GitPanelViewOptions { tree_view: true, ..state });
                        window.dispatch_action(Box::new(ToggleTreeView), cx);
                    }
                })
            })
            .when(!state.tree_view, |this| {
                this.separator()
                    .header("Sort By")
                    .item({
                        let view_options_menu_state = Rc::clone(&view_options_menu_state);
                        ContextMenuEntry::new("Path")
                            .toggle(IconPosition::End, state.sort_by == GitPanelSortBy::Path)
                            .handler(move |window, cx| {
                                if !state.tree_view {
                                    view_options_menu_state
                                        .set(GitPanelViewOptions { sort_by: GitPanelSortBy::Path, ..state });
                                    window.dispatch_action(Box::new(SetSortByPath), cx);
                                }
                            })
                    })
                    .item({
                        let view_options_menu_state = Rc::clone(&view_options_menu_state);
                        ContextMenuEntry::new("Name")
                            .toggle(IconPosition::End, state.sort_by == GitPanelSortBy::Name)
                            .handler(move |window, cx| {
                                if !state.tree_view {
                                    view_options_menu_state
                                        .set(GitPanelViewOptions { sort_by: GitPanelSortBy::Name, ..state });
                                    window.dispatch_action(Box::new(SetSortByName), cx);
                                }
                            })
                    })
            })
            .separator()
            .header("Group By")
            .item({
                let view_options_menu_state = Rc::clone(&view_options_menu_state);
                ContextMenuEntry::new("None")
                    .toggle(IconPosition::End, state.group_by == GitPanelGroupBy::None)
                    .handler(move |window, cx| {
                        if state.group_by != GitPanelGroupBy::None {
                            view_options_menu_state
                                .set(GitPanelViewOptions { group_by: GitPanelGroupBy::None, ..state });
                            window.dispatch_action(Box::new(SetGroupByNone), cx);
                        }
                    })
            })
            .item({
                let view_options_menu_state = Rc::clone(&view_options_menu_state);
                ContextMenuEntry::new("Tracked & Untracked")
                    .toggle(IconPosition::End, state.group_by == GitPanelGroupBy::Status)
                    .handler(move |window, cx| {
                        if state.group_by != GitPanelGroupBy::Status {
                            view_options_menu_state
                                .set(GitPanelViewOptions { group_by: GitPanelGroupBy::Status, ..state });
                            window.dispatch_action(Box::new(SetGroupByStatus), cx);
                        }
                    })
            })
            .item({
                let view_options_menu_state = Rc::clone(&view_options_menu_state);
                ContextMenuEntry::new("Staged & Unstaged")
                    .toggle(IconPosition::End, state.group_by == GitPanelGroupBy::Staging)
                    .handler(move |window, cx| {
                        if state.group_by != GitPanelGroupBy::Staging {
                            view_options_menu_state
                                .set(GitPanelViewOptions { group_by: GitPanelGroupBy::Staging, ..state });
                            window.dispatch_action(Box::new(SetGroupByStaging), cx);
                        }
                    })
            })
    })
}

pub(super) struct GitPanelContextMenu {
    menu: Entity<ContextMenu>,
    position: Point<Pixels>,
    pub(super) target_entry_index: Option<usize>,
    _subscription: Subscription,
}

impl Render for GitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_entries = !self.entries.is_empty();

        v_flex()
            .id("git_panel")
            .key_context(self.dispatch_context(window, cx))
            .track_focus(&self.focus_handle)
            .map(|this| {
                this.on_action(cx.listener(Self::toggle_staged_for_selected))
                    .on_action(cx.listener(Self::stage_range))
                    .on_action(cx.listener(GitPanel::on_commit))
                    .on_action(cx.listener(GitPanel::toggle_skip_hooks))
                    .on_action(cx.listener(GitPanel::on_amend))
                    .on_action(cx.listener(GitPanel::toggle_signoff_enabled))
                    .on_action(cx.listener(Self::stage_all))
                    .on_action(cx.listener(Self::unstage_all))
                    .on_action(cx.listener(Self::stage_selected))
                    .on_action(cx.listener(Self::unstage_selected))
                    .on_action(cx.listener(Self::stage_section))
                    .on_action(cx.listener(Self::unstage_section))
                    .on_action(cx.listener(Self::restore_tracked_files))
                    .on_action(cx.listener(Self::revert_selected))
                    .on_action(cx.listener(Self::add_to_gitignore))
                    .on_action(cx.listener(Self::add_to_git_info_exclude))
                    .on_action(cx.listener(Self::clean_all))
                    .on_action(cx.listener(Self::stash_all))
                    .on_action(cx.listener(Self::stash_tracked))
                    .on_action(cx.listener(Self::stash_staged))
                    .on_action(cx.listener(Self::stash_pop))
            })
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::collapse_selected_entry))
            .on_action(cx.listener(Self::expand_selected_entry))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::first_entry))
            .on_action(cx.listener(Self::next_entry))
            .on_action(cx.listener(Self::previous_entry))
            .on_action(cx.listener(Self::last_entry))
            .on_action(cx.listener(Self::close_panel))
            .on_action(cx.listener(Self::open_diff))
            .on_action(cx.listener(Self::open_solo_diff))
            .on_action(cx.listener(Self::view_file))
            .on_action(cx.listener(Self::view_file_history))
            .on_action(cx.listener(Self::copy_path))
            .on_action(cx.listener(Self::copy_relative_path))
            .on_action(cx.listener(Self::view_unstaged_changes))
            .on_action(cx.listener(Self::view_staged_changes))
            .on_action(cx.listener(Self::focus_changes_list))
            .on_action(cx.listener(Self::focus_editor))
            .on_action(cx.listener(Self::toggle_commit_editor))
            .on_action(cx.listener(Self::set_sort_by_path))
            .on_action(cx.listener(Self::set_sort_by_name))
            .on_action(cx.listener(Self::set_group_by_none))
            .on_action(cx.listener(Self::set_group_by_status))
            .on_action(cx.listener(Self::set_group_by_staging))
            .on_action(cx.listener(Self::toggle_tree_view))
            .on_action(cx.listener(Self::increase_font_size))
            .on_action(cx.listener(Self::decrease_font_size))
            .on_action(cx.listener(Self::reset_font_size))
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().colors().panel_background)
            .child(
                v_flex()
                    .size_full()
                    .children(self.render_changes_header(window, cx))
                    .when(!self.commit_editor_expanded, |this| {
                        this.map(|this| {
                            if let Some(repo) = self.active_repository.clone()
                                && has_entries
                            {
                                this.child(self.render_entries(repo, window, cx))
                            } else {
                                this.child(self.render_empty_state(cx).into_any_element())
                            }
                        })
                    })
                    .children(self.render_footer(window, cx))
                    .when(self.amend_pending, |this| this.child(self.render_pending_amend(cx)))
                    .when(!self.amend_pending, |this| this.children(self.render_previous_commit(window, cx)))
                    .into_any_element(),
            )
            .children(self.context_menu.as_ref().map(|context_menu| {
                deferred(
                    anchored().position(context_menu.position).anchor(Anchor::TopLeft).child(context_menu.menu.clone()),
                )
                .with_priority(1)
            }))
    }
}

struct GitPanelMessageTooltip {
    commit_tooltip: Option<Entity<CommitTooltip>>,
}

impl GitPanelMessageTooltip {
    fn new(
        git_panel: Entity<GitPanel>,
        sha: SharedString,
        repository: Entity<Repository>,
        window: &Window,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.spawn_in(window, async move |this, cx| {
                let (details, workspace) = git_panel.update(cx, |git_panel, cx| {
                    (git_panel.load_commit_details(sha.to_string(), cx), git_panel.workspace.clone())
                });
                let details = details.await?;

                let commit_details = commit_tooltip::CommitDetails {
                    sha: details.sha.clone(),
                    author_name: details.author_name.clone(),
                    author_email: details.author_email.clone(),
                    commit_time: OffsetDateTime::from_unix_timestamp(details.commit_timestamp)?,
                    message: Some(ParsedCommitMessage { message: details.message }),
                    tag_names: Vec::new(),
                    boundary: false,
                };

                this.update(cx, |this: &mut GitPanelMessageTooltip, cx| {
                    this.commit_tooltip =
                        Some(cx.new(move |cx| CommitTooltip::new(commit_details, repository, workspace, cx)));
                    cx.notify();
                })
            })
            .detach();

            Self { commit_tooltip: None }
        })
    }
}

impl Render for GitPanelMessageTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(commit_tooltip) = &self.commit_tooltip {
            commit_tooltip.clone().into_any_element()
        } else {
            gpui::Empty.into_any_element()
        }
    }
}

impl GitPanel {
    /// Computes tree indentation depths for visible entries in the given range.
    /// Used by indent guides to render vertical connector lines in tree view.
    fn compute_visible_depths(&self, range: Range<usize>) -> SmallVec<[usize; 64]> {
        let GitPanelViewMode::Tree(state) = &self.view_mode else {
            return SmallVec::new();
        };

        range
            .map(|ix| {
                state
                    .logical_indices
                    .get(ix)
                    .and_then(|&entry_ix| self.entries.get(entry_ix))
                    .map_or(0, |entry| entry.depth())
            })
            .collect()
    }

    fn status_width_estimate(tree_view: bool, entry: &GitStatusEntry, depth: usize) -> usize {
        if tree_view {
            Self::item_width_estimate(0, entry.display_name().len(), depth)
        } else {
            Self::item_width_estimate(entry.parent_dir().map_or(0, |s| s.len()), entry.display_name().len(), 0)
        }
    }

    pub(super) fn width_estimate_for_list_entry(&self, tree_view: bool, entry: &GitListEntry) -> Option<usize> {
        match entry {
            GitListEntry::Status(status) => Some(Self::status_width_estimate(tree_view, status, 0)),
            GitListEntry::TreeStatus(status) => {
                Some(Self::status_width_estimate(tree_view, &status.entry, status.depth))
            }
            GitListEntry::Directory(dir) => Some(Self::item_width_estimate(0, dir.name.len(), dir.depth)),
            GitListEntry::Header(_) | GitListEntry::EmptySection(_) => None,
        }
    }

    const fn item_width_estimate(path: usize, file_name: usize, depth: usize) -> usize {
        path + file_name + depth * 2
    }

    fn render_view_options_menu(&self, id: impl Into<ElementId>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();

        PopoverMenu::new(id.into())
            .trigger_with_tooltip(
                IconButton::new("view-options-menu-trigger", IconName::Filter).icon_size(IconSize::Small),
                Tooltip::text("View Options"),
            )
            .menu(move |window, cx| Some(git_panel_view_options_menu(focus_handle.clone(), window, cx)))
            .anchor(Anchor::TopRight)
    }

    fn render_git_commit_menu(
        &self,
        id: impl Into<ElementId>,
        keybinding_target: Option<FocusHandle>,
        disabled: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let menu_open = self.commit_menu_handle.is_deployed();

        PopoverMenu::new(id.into())
            .trigger(
                crate::render_split_button_chevron_trigger("commit-split-button-right", menu_open).disabled(disabled),
            )
            .with_handle(self.commit_menu_handle.clone())
            .menu({
                // Weak: the menu handle the panel owns keeps this builder.
                let git_panel = cx.weak_entity();
                let has_previous_commit = self.head_commit(cx).is_some();
                let amend = self.amend_pending();
                let signoff = self.signoff_enabled;
                let skip_hooks = self.skip_hooks_enabled;

                move |window, cx| {
                    Some(ContextMenu::build(window, cx, |context_menu, _, _| {
                        context_menu
                            .when_some(keybinding_target.clone(), |el, keybinding_target| el.context(keybinding_target))
                            .when(has_previous_commit, |this| {
                                this.toggleable_entry("Amend", amend, IconPosition::Start, Some(Box::new(Amend)), {
                                    let git_panel = git_panel.clone();
                                    move |_, cx| {
                                        git_panel
                                            .update(cx, |git_panel, cx| {
                                                git_panel.toggle_amend_pending(cx);
                                            })
                                            .ok();
                                    }
                                })
                            })
                            .toggleable_entry(
                                "Signoff",
                                signoff,
                                IconPosition::Start,
                                Some(Box::new(Signoff)),
                                move |window, cx| window.dispatch_action(Box::new(Signoff), cx),
                            )
                            .item(
                                ContextMenuEntry::new("Skip Hooks")
                                    .toggleable(IconPosition::Start, skip_hooks)
                                    .action(Box::new(SkipHooks))
                                    .handler(move |window, cx| window.dispatch_action(Box::new(SkipHooks), cx))
                                    .documentation_aside(DocumentationSide::Left, |_| {
                                        Label::new("git commit --no-verify").into_any_element()
                                    }),
                            )
                    }))
                }
            })
            .anchor(Anchor::TopRight)
            .offset(gpui::Point { x: px(0.), y: px(2.) })
    }

    fn render_git_changes_actions_menu(&self, id: impl Into<ElementId>, cx: &Context<Self>) -> impl IntoElement {
        let has_tracked_changes = self.has_tracked_changes();
        let has_staged_tracked_changes = self.has_staged_tracked_changes();
        let has_staged_changes = self.has_staged_changes();
        let has_unstaged_changes = self.has_unstaged_changes();
        let has_new_changes = self.new_count > 0;
        let has_stash_items = !self.stash_entries.entries.is_empty();
        let can_stash = !self.rebase_in_progress(cx);
        let group_by = GitPanelViewOptions::get(cx).group_by;

        let focus_handle = self.focus_handle.clone();
        let menu_open = self.changes_actions_menu_handle.is_deployed();

        PopoverMenu::new(id.into())
            .trigger(crate::render_split_button_chevron_trigger("changes-actions-split-button-right", menu_open))
            .with_handle(self.changes_actions_menu_handle.clone())
            .menu(move |window, cx| {
                Some(git_panel_context_menu(
                    has_tracked_changes,
                    has_staged_tracked_changes,
                    has_staged_changes,
                    has_unstaged_changes,
                    has_new_changes,
                    has_stash_items,
                    can_stash,
                    group_by,
                    false,
                    focus_handle.clone(),
                    window,
                    cx,
                ))
            })
            .anchor(Anchor::TopRight)
            .offset(gpui::Point { x: px(0.), y: px(2.) })
    }

    fn render_git_changes_actions_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let (text, action, stage, tooltip) = if self.primary_changes_action_stages() {
            ("Stage All", StageAll.boxed_clone(), true, "git add --all")
        } else {
            ("Unstage All", UnstageAll.boxed_clone(), false, "git reset")
        };

        SplitButton::new(
            ButtonLike::new_rounded_left("git-changes-actions-split-button-left")
                .layer(ElevationIndex::ModalSurface)
                .size(ButtonSize::Compact)
                .child(Label::new(text).size(LabelSize::Small).mr_0p5())
                .tooltip(Tooltip::for_action_title_in(tooltip, action.as_ref(), &self.focus_handle))
                .disabled(self.entry_count == 0)
                .on_click({
                    let git_panel = cx.weak_entity();
                    move |_, _, cx| {
                        git_panel
                            .update(cx, |git_panel, cx| {
                                git_panel.change_all_files_stage(stage, cx);
                            })
                            .ok();
                    }
                }),
            self.render_git_changes_actions_menu("git-changes-actions-split-button-menu", cx).into_any_element(),
        )
    }

    fn render_changes_header(&self, _window: &mut Window, cx: &Context<Self>) -> Option<impl IntoElement> {
        if matches!(self.git_access, Some(GitAccess::No)) {
            return None;
        }

        self.active_repository.as_ref()?;

        let diff_stat_total = self.diff_stat_total;

        Some(
            h_flex()
                .min_h(Tab::container_height(cx))
                .w_full()
                .pl_1()
                .pr_2()
                .flex_none()
                .flex_wrap()
                .gap_1()
                .justify_between()
                .child(
                    ButtonLike::new("diff-button")
                        .child(
                            h_flex()
                                .gap_1()
                                .child(Icon::new(IconName::Diff).size(IconSize::Small).color(Color::Muted))
                                .child(Label::new("View Diff").size(LabelSize::Small).color(Color::Muted))
                                .when(diff_stat_total != DiffStat::default(), |this| {
                                    this.child(ui::DiffStat::new(
                                        "changes-diff-stat-total",
                                        diff_stat_total.added as usize,
                                        diff_stat_total.deleted as usize,
                                    ))
                                }),
                        )
                        .tooltip(Tooltip::for_action_title_in("View Diff", &Diff, &self.focus_handle))
                        .on_click(|_, _, cx| {
                            cx.defer(|cx| {
                                cx.dispatch_action(&Diff);
                            });
                        }),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(self.render_view_options_menu("view_options_menu"))
                        .child(self.render_git_changes_actions_button(cx)),
                ),
        )
    }

    pub(crate) fn render_remote_button(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let branch = self.active_repository.as_ref()?.read(cx).branch.clone();
        if !self.can_push_and_pull(cx) {
            return None;
        }
        Some(
            h_flex()
                .gap_1()
                .flex_shrink_0()
                .when_some(branch, |this, branch| {
                    let focus_handle = Some(self.focus_handle(cx));

                    this.children(render_remote_button(
                        "remote-button",
                        &branch,
                        focus_handle,
                        true,
                        self.pending_remote_operation,
                        self.remote_action_menu_handle.clone(),
                    ))
                })
                .into_any_element(),
        )
    }

    /// Takes the remote button's place while a rebase is stopped on conflicts.
    fn render_rebase_row(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.rebase_in_progress(cx) {
            return None;
        }
        let busy = self.pending_remote_operation.is_some();
        let unresolved = self.has_unstaged_conflicts();
        Some(
            h_flex()
                .w_full()
                .px_2()
                .py_1p5()
                .gap_1()
                .justify_between()
                .child(
                    div()
                        .id("rebase-in-progress")
                        .child(Label::new("Rebase in progress").size(LabelSize::Small).color(Color::Warning))
                        .tooltip(Tooltip::text(
                            "Changes that were uncommitted before the pull come back when the rebase ends",
                        )),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("rebase-abort", "Abort")
                                .label_size(LabelSize::Small)
                                .disabled(busy)
                                .tooltip(Tooltip::text("git rebase --abort"))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.rebase(RebaseAction::Abort, window, cx)),
                                ),
                        )
                        .child(
                            Button::new("rebase-continue", "Continue")
                                .style(ButtonStyle::Filled)
                                .label_size(LabelSize::Small)
                                .disabled(busy || unresolved)
                                .tooltip(Tooltip::text(if unresolved {
                                    "Stage the resolved conflicts first"
                                } else {
                                    "git rebase --continue"
                                }))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.rebase(RebaseAction::Continue, window, cx)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    pub fn render_footer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.active_repository.as_ref()?;
        let panel_editor_style = git_commit_editor_style(git_commit_font_size(cx), cx);

        let editor_is_long =
            self.commit_editor.update(cx, |editor, cx| editor.max_point(cx).row().0 >= MAX_PANEL_EDITOR_LINES as u32);

        let vertical_buttons = v_flex().h_full().p_1p5().child({
            let expanded = self.commit_editor_expanded;
            let label = if expanded { "Collapse Commit Editor" } else { "Expand Commit Editor" };
            let focus_handle = self.focus_handle.clone();

            IconButton::new("fill-commit-editor", IconName::Maximize)
                .icon_size(IconSize::Small)
                .icon_color(Color::Hidden)
                .hover_color(Color::Muted)
                .toggle_state(expanded)
                .selected_icon(IconName::Minimize)
                .tooltip({
                    move |_window, cx| Tooltip::for_action_in(label, &git::ToggleFillCommitEditor, &focus_handle, cx)
                })
                .on_click(cx.listener({
                    move |_, _, window, cx| window.dispatch_action(git::ToggleFillCommitEditor.boxed_clone(), cx)
                }))
        });

        let collapsed = self.commit_editor_collapsed;
        let toggle_commit_editor_button = {
            let (icon, label) = if collapsed {
                (IconName::ChevronUp, "Show Commit Editor")
            } else {
                (IconName::ChevronDown, "Hide Commit Editor")
            };
            let focus_handle = self.focus_handle.clone();

            IconButton::new("toggle-commit-editor", icon)
                .icon_size(IconSize::Small)
                .tooltip(move |_window, cx| Tooltip::for_action_in(label, &git::ToggleCommitEditor, &focus_handle, cx))
                .on_click(cx.listener(move |_, _, window, cx| {
                    window.dispatch_action(git::ToggleCommitEditor.boxed_clone(), cx);
                }))
        };

        let footer = v_flex()
            .when(self.commit_editor_expanded, |this| this.flex_1().min_h_0())
            // The repository and branch are named by the window header right
            // above, so this row only holds the remote actions.
            .children(self.render_rebase_row(cx).or_else(|| {
                self.render_remote_button(cx).map(|remote_button| {
                    h_flex().w_full().px_2().py_1p5().justify_end().child(remote_button).into_any_element()
                })
            }))
            .child(
                panel_editor_container(window, cx)
                    .id("commit-editor-container")
                    .when(self.commit_editor_expanded, |this| this.flex_1().min_h_0())
                    // A floating card like the editor panes. It spans the dock, so the window edge and
                    // the dock's gap to the center give it the same 8px gaps as every other card.
                    .rounded(CARD_RADIUS)
                    .overflow_hidden()
                    .when(!collapsed, |this| {
                        this.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            window.focus(&this.commit_editor.focus_handle(cx), cx);
                        }))
                        .child(
                            h_flex()
                                .size_full()
                                .child(
                                    div()
                                        .pt_2()
                                        .px_2()
                                        .h_full()
                                        .flex_grow_1()
                                        .cursor_text()
                                        .on_action(|&MoveUp, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_action(|&MoveDown, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .child(EditorElement::new(&self.commit_editor, panel_editor_style)),
                                )
                                .child(vertical_buttons),
                        )
                    })
                    .child(
                        h_flex()
                            .id("commit-footer")
                            .w_full()
                            .p_1p5()
                            .border_t_1()
                            .when(editor_is_long, |el| el.border_color(cx.theme().colors().border_variant))
                            .justify_between()
                            .child(h_flex().gap_0p5().child(toggle_commit_editor_button))
                            .child(h_flex().gap_0p5().child(self.render_commit_button(cx))),
                    ),
            );

        Some(footer)
    }

    fn render_commit_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (can_commit, tooltip) = self.configure_commit_button(cx);
        let title = self.commit_button_title();
        let commit_tooltip_focus_handle = self.commit_editor.focus_handle(cx);
        let options = self.commit_options();
        let amend = options.amend;
        let signoff = options.signoff;
        let no_verify = options.no_verify;

        let label_color = if self.pending_commit.is_some() { Color::Disabled } else { Color::Default };

        h_flex()
            .id("commit-wrapper")
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                this.show_placeholders = *hovered && !this.has_staged_changes() && !this.has_unstaged_conflicts();
                cx.notify();
            }))
            .child(SplitButton::new(
                ButtonLike::new_rounded_left(format!("split-button-left-{title}"))
                    .layer(ElevationIndex::ModalSurface)
                    .size(ButtonSize::Compact)
                    .disabled(!can_commit)
                    .child(Label::new(title).size(LabelSize::Small).color(label_color).mr_0p5())
                    .on_click({
                        let git_panel = cx.weak_entity();
                        move |_, window, cx| {
                            git_panel
                                .update(cx, |git_panel, cx| {
                                    let options = git_panel.commit_options();
                                    git_panel.commit_changes(options, window, cx);
                                })
                                .ok();
                        }
                    })
                    .tooltip({
                        let handle = commit_tooltip_focus_handle.clone();
                        move |_window, cx| {
                            if can_commit {
                                Tooltip::with_meta_in(
                                    tooltip,
                                    Some(&git::Commit),
                                    format!(
                                        "git commit{}{}{}",
                                        if amend { " --amend" } else { "" },
                                        if signoff { " --signoff" } else { "" },
                                        if no_verify { " --no-verify" } else { "" }
                                    ),
                                    &handle.clone(),
                                    cx,
                                )
                            } else {
                                Tooltip::simple(tooltip, cx)
                            }
                        }
                    }),
                self.render_git_commit_menu(
                    ElementId::Name(format!("split-button-right-{title}").into()),
                    Some(commit_tooltip_focus_handle),
                    false,
                    cx,
                )
                .into_any_element(),
            ))
    }

    fn render_pending_amend(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .py_1p5()
            .px_2()
            .gap_1p5()
            .justify_between()
            .child(
                div()
                    .flex_grow_1()
                    .overflow_hidden()
                    .max_w(relative(0.85))
                    .child(Label::new("This will update your most recent commit.").size(LabelSize::Small).truncate()),
            )
            .child(
                Button::new("cancel", "Cancel")
                    .label_size(LabelSize::Small)
                    .layer(ElevationIndex::ModalSurface)
                    .on_click(cx.listener(|this, _, _, cx| this.set_amend_pending(false, cx))),
            )
    }

    fn render_previous_commit(&self, _window: &mut Window, cx: &Context<Self>) -> Option<impl IntoElement> {
        let active_repository = self.active_repository.as_ref()?;
        let branch = active_repository.read(cx).branch.as_ref()?;
        let commit = branch.most_recent_commit.as_ref()?.clone();
        let workspace = self.workspace.clone();
        let this = cx.entity();

        Some(
            h_flex()
                .p_1p5()
                .gap_1p5()
                .justify_between()
                .child(
                    div()
                        .id("commit-msg-hover")
                        .cursor_pointer()
                        .px_1()
                        .rounded_sm()
                        .line_clamp(1)
                        .hover(|s| s.bg(cx.theme().colors().element_hover))
                        .child(Label::new(commit.subject.clone()).size(LabelSize::Small).truncate())
                        .on_click({
                            let commit = commit.clone();
                            let repo = active_repository.downgrade();
                            move |_, window, cx| {
                                CommitView::open(
                                    commit.sha.to_string(),
                                    repo.clone(),
                                    workspace.clone(),
                                    None,
                                    None,
                                    window,
                                    cx,
                                );
                            }
                        })
                        .hoverable_tooltip({
                            let repo = active_repository.clone();
                            move |window, cx| {
                                GitPanelMessageTooltip::new(this.clone(), commit.sha.clone(), repo.clone(), window, cx)
                                    .into()
                            }
                        }),
                )
                .child(h_flex().gap_0p5().when(commit.has_parent, |this| {
                    let has_unstaged = self.has_unstaged_changes();
                    this.child(
                        IconButton::new("undo", IconName::Undo)
                            .icon_size(IconSize::Small)
                            .disabled(self.rebase_in_progress(cx))
                            .tooltip(move |_window, cx| {
                                Tooltip::with_meta(
                                    "Uncommit",
                                    Some(&git::Uncommit),
                                    if has_unstaged { "git reset HEAD^ --soft" } else { "git reset HEAD^" },
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.uncommit(window, cx))),
                    )
                })),
        )
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match (self.git_access, &self.active_repository) {
            (Some(GitAccess::No), Some(repository)) => self.render_unsafe_repo_ui(repository, cx),
            (_, None) => self.render_uninitialized_ui(cx),
            (_, Some(_)) => self.render_no_changes_ui(cx),
        };

        v_flex().gap_1p5().flex_1().items_center().justify_center().child(content)
    }

    fn render_no_changes_ui(&self, cx: &Context<Self>) -> AnyElement {
        let show_branch_diff = self.changes_count == 0 && !self.is_on_main_branch(cx);

        v_flex()
            .gap_1()
            .items_center()
            .child(Label::new("No changes to commit").color(Color::Muted))
            .when(show_branch_diff, |this| {
                this.child(
                    Button::new("view_branch_diff", "View Branch Diff")
                        .label_size(LabelSize::Small)
                        .style(ButtonStyle::Outlined)
                        .on_click(move |_, _, cx| {
                            cx.defer(move |cx| {
                                cx.dispatch_action(&DeployBranchDiff);
                            });
                        }),
                )
            })
            .into_any_element()
    }

    fn render_unsafe_repo_ui(&self, active_repository: &Entity<Repository>, cx: &mut Context<Self>) -> AnyElement {
        let directory = active_repository.update(cx, |repository, _cx| repository.snapshot().work_directory_abs_path);

        let message = format!(
            "Detected dubious ownership in repository at {}. \
            This happens when the .git/ directory is not owned by the current user. \
            If you want to learn more about safe directories, visit git's documentation.",
            directory.display()
        );

        v_flex()
            .px_4()
            .gap_1()
            .child(Label::new(message).color(Color::Muted))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new("trust_directory", "Trust Directory")
                            .label_size(LabelSize::Small)
                            .layer(ElevationIndex::ModalSurface)
                            .style(ButtonStyle::Filled)
                            .tooltip(Tooltip::text(format!(
                                "git config --global --add safe.directory {}",
                                directory.display()
                            )))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_safe_directory(window, cx);
                            })),
                    )
                    .child(
                        Button::new("learn_more", "Learn More")
                            .label_size(LabelSize::Small)
                            .style(ButtonStyle::Outlined)
                            .end_icon(Icon::new(IconName::ArrowUpRight).size(IconSize::Small).color(Color::Muted))
                            .on_click(move |_, _, cx| {
                                cx.open_url(
                                    "https://git-scm.com/docs/git-config#Documentation/git-config.txt-safedirectory",
                                );
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_uninitialized_ui(&self, cx: &Context<Self>) -> AnyElement {
        let project = self.project.read(cx);
        let worktree_count = project.visible_worktrees(cx).count();
        // `git init` can't run inside a worktree whose root is a single file.
        let has_directory_worktree = project.visible_worktrees(cx).any(|worktree| !worktree.read(cx).is_single_file());
        if worktree_count > 0 && self.active_repository.is_none() {
            v_flex()
                .gap_1()
                .items_center()
                .child(Label::new("No Git Repositories").color(Color::Muted))
                .when(has_directory_worktree, |this| {
                    this.child(
                        Button::new("initialize_repository", "Initialize Repository")
                            .label_size(LabelSize::Small)
                            .style(ButtonStyle::Outlined)
                            .tooltip(Tooltip::for_action_title_in("git init", &git::Init, &self.focus_handle))
                            .on_click(move |_, _, cx| {
                                cx.defer(move |cx| {
                                    cx.dispatch_action(&git::Init);
                                });
                            }),
                    )
                })
                .into_any_element()
        } else if worktree_count == 0 {
            let focus_handle = self.focus_handle.clone();
            ProjectEmptyState::new(
                "Git Panel",
                focus_handle.clone(),
                KeyBinding::for_action_in(&workspace::Open::default(), &focus_handle, cx),
            )
            .on_open_project(|_, window, cx| {
                window.dispatch_action(workspace::Open::default().boxed_clone(), cx);
            })
            .on_clone_repo(|_, window, cx| {
                window.dispatch_action(git::Clone.boxed_clone(), cx);
            })
            .into_any_element()
        } else {
            Empty.into_any_element()
        }
    }

    fn is_on_main_branch(&self, cx: &Context<Self>) -> bool {
        let Some(repo) = self.active_repository.as_ref() else {
            return false;
        };

        let Some(branch) = repo.read(cx).branch.as_ref() else {
            return false;
        };

        let branch_name = branch.name();
        matches!(branch_name, "main" | "master")
    }

    pub(super) fn render_buffer_header_controls(
        &self,
        entity: &Entity<Self>,
        file: &Arc<dyn File>,
        _: &Window,
        cx: &App,
    ) -> Option<AnyElement> {
        let repo = self.active_repository.as_ref()?.read(cx);
        let project_path = (file.worktree_id(cx), Arc::clone(file.path())).into();
        let repo_path = repo.project_path_to_repo_path(&project_path, cx)?;
        let ix = self.entry_by_path(&repo_path)?;
        let entry = self.entries.get(ix)?;

        let is_staging_or_staged = repo
            .pending_ops_for_path(&repo_path)
            .map(|ops| !ops.last_op_errored() && (ops.staging() || ops.staged()))
            .or_else(|| {
                let status = repo.status_for_path(&repo_path)?;
                status.status.staging().as_bool()
            })
            .or_else(|| {
                let entry = entry.status_entry()?;
                entry.staging.as_bool()
            });

        let checkbox = Checkbox::new("stage-file", is_staging_or_staged.into())
            .fill()
            .elevation(ElevationIndex::Surface)
            .on_click({
                let entry = entry.clone();
                let git_panel = entity.downgrade();
                move |_, window, cx| {
                    git_panel
                        .update(cx, |this, cx| {
                            this.toggle_staged_for_entry(&entry, StageIntent::Toggle, window, cx);
                            cx.stop_propagation();
                        })
                        .ok();
                }
            });
        Some(
            h_flex()
                .id("start-slot")
                .text_lg()
                .child(checkbox)
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    // prevent the list item active state triggering when toggling checkbox
                    cx.stop_propagation();
                })
                .into_any_element(),
        )
    }

    fn render_entries(
        &self,
        repo: Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (is_tree_view, entry_count) = match &self.view_mode {
            GitPanelViewMode::Tree(state) => (true, state.logical_indices.len()),
            GitPanelViewMode::Flat => (false, self.visible_flat_entry_indices().len()),
        };
        let repo = repo.downgrade();

        v_flex().flex_1().size_full().overflow_hidden().relative().child(
            h_flex()
                .flex_1()
                .size_full()
                .relative()
                .overflow_hidden()
                .child(
                    uniform_list(
                        "entries",
                        entry_count,
                        cx.processor(move |this, range: Range<usize>, window, cx| {
                            let Some(repo) = repo.upgrade() else {
                                return Vec::new();
                            };
                            let repo = repo.read(cx);

                            let mut items = Vec::with_capacity(range.end - range.start);

                            let visible_flat_entry_indices = matches!(this.view_mode, GitPanelViewMode::Flat)
                                .then(|| this.visible_flat_entry_indices());
                            for ix in range.into_iter().filter_map(|ix| match &this.view_mode {
                                GitPanelViewMode::Tree(state) => state.logical_indices.get(ix).copied(),
                                GitPanelViewMode::Flat => {
                                    visible_flat_entry_indices.as_ref().and_then(|indices| indices.get(ix)).copied()
                                }
                            }) {
                                match &this.entries.get(ix) {
                                    Some(GitListEntry::Status(entry)) => {
                                        items.push(this.render_status_entry(ix, entry, 0, repo, window, cx));
                                    }
                                    Some(GitListEntry::TreeStatus(entry)) => {
                                        items.push(this.render_status_entry(
                                            ix,
                                            &entry.entry,
                                            entry.depth,
                                            repo,
                                            window,
                                            cx,
                                        ));
                                    }
                                    Some(GitListEntry::Directory(entry)) => {
                                        items.push(this.render_directory_entry(ix, entry, window, cx));
                                    }
                                    Some(GitListEntry::Header(header)) => {
                                        items.push(this.render_list_header(ix, header, window, cx));
                                    }
                                    Some(GitListEntry::EmptySection(section)) => {
                                        items.push(this.render_empty_section(*section));
                                    }
                                    None => {}
                                }
                            }

                            items
                        }),
                    )
                    .when(is_tree_view, |list| {
                        list.with_decoration(
                            ui::indent_guides(px(TREE_INDENT), IndentGuideColors::panel(cx))
                                .with_left_offset(INDENT_GUIDE_LEFT_OFFSET)
                                .with_compute_indents_fn(cx.entity(), |this, range, _window, _cx| {
                                    this.compute_visible_depths(range)
                                }),
                        )
                    })
                    .group("entries")
                    .size_full()
                    .flex_grow_1()
                    .with_width_from_item(self.max_width_item_index)
                    .track_scroll(&self.scroll_handle),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.deploy_panel_context_menu(event.position, None, false, window, cx);
                    }),
                )
                .custom_scrollbars(
                    Scrollbars::new(ScrollAxes::Both)
                        .tracked_scroll_handle(&self.scroll_handle)
                        .with_track_along(ScrollAxes::Horizontal, cx.theme().colors().panel_background),
                    window,
                    cx,
                ),
        )
    }

    fn entry_label(&self, label: impl Into<SharedString>, color: Color) -> Label {
        Label::new(label.into()).single_line().color(color)
    }

    const fn list_item_height(&self) -> Rems {
        LIST_ITEM_HEIGHT
    }

    fn render_list_header(
        &self,
        ix: usize,
        header: &GitHeaderEntry,
        _window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = ElementId::Name(format!("header_{ix}").into());
        let checkbox_id = ElementId::Name(format!("header_{ix}_checkbox").into());
        let group_name: SharedString = format!("header_{ix}").into();
        let section = header.header;
        let weak = cx.weak_entity();
        let checkbox_weak = weak.clone();
        let stage_intent = StageIntent::for_section(section);
        let toggle_state = stage_intent.checkbox_state(|| self.header_state(header.header));
        let is_collapsed = self.collapsed_sections.contains(&section);

        let all_conflicts_resolved = section == Section::Conflict
            && self.conflicted_count > 0
            && self.conflicted_staged_count == self.conflicted_count;

        let section_is_empty = !self.entries.get(ix + 1).is_some_and(GitListEntry::is_selectable);
        let file_count = self.section_file_counts.get(&section).copied().unwrap_or_default();

        h_flex()
            .id(id)
            .group(group_name)
            .h(self.list_item_height())
            .w_full()
            .pl_2p5()
            .pr_1()
            .gap_2()
            .justify_between()
            .cursor_pointer()
            .hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
            .border_1()
            .border_r_2()
            .child(
                h_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        Icon::new(if is_collapsed { IconName::ChevronRight } else { IconName::ChevronDown })
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(Label::new(header.title()).color(Color::Muted).size(LabelSize::Small)),
            )
            .when(file_count > 0, |this| this.child(Chip::new(file_count.to_string()).label_color(Color::Muted)))
            .child(if section_is_empty {
                gpui::Empty.into_any_element()
            } else {
                let checkbox = Checkbox::new(checkbox_id, toggle_state)
                    .disabled(all_conflicts_resolved)
                    .fill()
                    .elevation(ElevationIndex::Surface)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        if all_conflicts_resolved {
                            return;
                        }

                        checkbox_weak
                            .update(cx, |this, cx| {
                                this.toggle_staged_for_entry(
                                    &GitListEntry::Header(GitHeaderEntry { header: section }),
                                    stage_intent,
                                    window,
                                    cx,
                                );
                            })
                            .ok();
                    });
                let tooltip_label = if all_conflicts_resolved {
                    Some("All conflicts marked as resolved")
                } else {
                    match stage_intent {
                        StageIntent::Stage => Some("Stage All"),
                        StageIntent::Unstage => Some("Unstage All"),
                        StageIntent::Toggle => None,
                    }
                };
                if let Some(label) = tooltip_label {
                    checkbox.tooltip(move |_window, cx| Tooltip::simple(label, cx)).into_any_element()
                } else {
                    checkbox.into_any_element()
                }
            })
            .on_click(move |_, window, cx| {
                weak.update(cx, |this, cx| {
                    this.toggle_section_collapsed(section, window, cx);
                })
                .ok();
            })
            .into_any_element()
    }

    fn render_empty_section(&self, section: Section) -> AnyElement {
        let message = match section {
            Section::Staged => "No staged changes yet",
            Section::Unstaged => "No unstaged changes",
            _ => "No changes",
        };
        h_flex()
            .h(self.list_item_height())
            .w_full()
            .pl_2p5()
            .pr_1()
            .opacity(0.8)
            .child(Label::new(message).color(Color::Placeholder).size(LabelSize::Small))
            .into_any_element()
    }

    fn build_context_menu(
        &self,
        will_unstage: bool,
        all_created: bool,
        all_deleted: bool,
        file_count: usize,
        target_kind: SelectionTargetKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        // It is possible for the `target_kind` to be `Multiple` but
        // `file_count` to be 1, for example, in the case where a marked
        // directory and its only single descendant are marked. In that case,
        // even though there's two rows marked, there's a single file that is
        // being affected.
        // As such, we need to ensure we show the correct label, depending on
        // the actual `file_count`.
        let file_label = if file_count == 1 { "File" } else { "Files" };

        let stage_title = match (target_kind, will_unstage) {
            (SelectionTargetKind::Directory, true) => "Unstage Folder",
            (SelectionTargetKind::Directory, false) => "Stage Folder",
            (SelectionTargetKind::File, true) => "Unstage File",
            (SelectionTargetKind::File, false) => "Stage File",
            (SelectionTargetKind::Multiple, true) => &format!("Unstage {file_count} {file_label}"),
            (SelectionTargetKind::Multiple, false) => &format!("Stage {file_count} {file_label}"),
        };

        let restore_title = restore_title(target_kind, all_created, all_deleted, file_count);
        let is_bulk = matches!(target_kind, SelectionTargetKind::Multiple);
        let is_file_or_bulk = !matches!(target_kind, SelectionTargetKind::Directory);
        let has_tracked = !all_created;
        let plural = file_count > 1;
        let copy_path_title = if plural { "Copy Paths" } else { "Copy Path" };
        let copy_relative_path_title = if plural { "Copy Relative Paths" } else { "Copy Relative Path" };
        let open_file_diff_title = if plural { "Open File Diffs" } else { "Open File Diff" };
        let view_file_title = if plural { "View Files" } else { "View File" };
        let open_diff_title = if plural { "Open Diffs" } else { "Open Diff" };
        let view_file_history_title = if plural { "View File Histories" } else { "View File History" };
        ContextMenu::build(window, cx, |context_menu, _, _| {
            context_menu
                .context(self.focus_handle.clone())
                .action(stage_title, ToggleStaged.boxed_clone())
                .action(restore_title.as_str(), RestoreFile::default().boxed_clone())
                .separator()
                .action("Unstaged Changes", ViewUnstagedChanges.boxed_clone())
                .action("Staged Changes", ViewStagedChanges.boxed_clone())
                .separator()
                .action(copy_path_title, CopyPath.boxed_clone())
                .action(copy_relative_path_title, CopyRelativePath.boxed_clone())
                .separator()
                .action_disabled_when(
                    !all_created,
                    if is_bulk && plural { "Add Files to .gitignore" } else { "Add to .gitignore" },
                    git::AddToGitignore.boxed_clone(),
                )
                .action_disabled_when(
                    !all_created,
                    if is_bulk && plural { "Add Files to .git/info/exclude" } else { "Add to .git/info/exclude" },
                    git::AddToGitInfoExclude.boxed_clone(),
                )
                .when(is_file_or_bulk, |context_menu| {
                    context_menu
                        .separator()
                        .action(open_diff_title, menu::Confirm.boxed_clone())
                        .action(open_file_diff_title, menu::SecondaryConfirm.boxed_clone())
                        .action(view_file_title, ViewFile.boxed_clone())
                })
                .when(is_file_or_bulk && has_tracked, |context_menu| {
                    context_menu.separator().action(view_file_history_title, Box::new(git::FileHistory))
                })
        })
    }

    fn deploy_entry_context_menu(
        &mut self,
        position: Point<Pixels>,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.row_is_marked(ix) {
            self.clear_marks();
        }

        self.selected_entry = Some(ix);

        let Some(target_kind) = self.selection_target_kind() else {
            return;
        };

        let entries = self.effective_status_entries();

        if entries.is_empty() {
            return;
        }

        let all_created = entries.iter().all(|entry| entry.status.is_created());
        let all_deleted = entries.iter().all(|entry| entry.status.is_deleted());
        let will_unstage = self.should_unstage(target_kind, &entries, cx);

        self.set_context_menu(
            self.build_context_menu(will_unstage, all_created, all_deleted, entries.len(), target_kind, window, cx),
            position,
            None,
            window,
            cx,
        );
    }

    fn deploy_panel_context_menu(
        &mut self,
        position: Point<Pixels>,
        target_entry_index: Option<usize>,
        include_copy_paths: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_stash_items = !self.stash_entries.entries.is_empty();
        let has_tracked_changes = self.has_tracked_changes();
        let has_staged_changes = self.has_staged_changes();
        let has_unstaged_changes = self.has_unstaged_changes();
        let has_new_changes = self.new_count > 0;
        let has_staged_tracked_changes = target_entry_index
            .and_then(|entry_index| self.directory_descendants(entry_index))
            .map_or_else(|| self.has_staged_tracked_changes(), Self::contains_staged_tracked_entry);

        let context_menu = git_panel_context_menu(
            has_tracked_changes,
            has_staged_tracked_changes,
            has_staged_changes,
            has_unstaged_changes,
            has_new_changes,
            has_stash_items,
            !self.rebase_in_progress(cx),
            GitPanelViewOptions::get(cx).group_by,
            include_copy_paths,
            self.focus_handle.clone(),
            window,
            cx,
        );

        self.set_context_menu(context_menu, position, target_entry_index, window, cx);
    }

    fn set_context_menu(
        &mut self,
        context_menu: Entity<ContextMenu>,
        position: Point<Pixels>,
        target_entry_index: Option<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let focus_handle = context_menu.focus_handle(cx);
        window.defer(cx, move |window, cx| {
            window.focus(&focus_handle, cx);
        });

        let subscription = cx.subscribe_in(&context_menu, window, |this, _, _: &DismissEvent, window, cx| {
            if this
                .context_menu
                .as_ref()
                .is_some_and(|context_menu| context_menu.menu.focus_handle(cx).contains_focused(window, cx))
            {
                this.activation_focus_handle(cx).focus(window, cx);
            }
            this.context_menu.take();
            cx.notify();
        });
        self.context_menu =
            Some(GitPanelContextMenu { menu: context_menu, position, target_entry_index, _subscription: subscription });
        cx.notify();
    }

    fn render_status_entry(
        &self,
        ix: usize,
        entry: &GitStatusEntry,
        depth: usize,
        repo: &Repository,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let tree_view = GitPanelViewOptions::get(cx).tree_view;
        let display_name = entry.display_name();

        let selected = self.selected_entry == Some(ix);
        let marked = self.marked_entries.contains(&entry.repo_path);
        let status = entry.status;
        let label_color = Color::Default;
        let path_color = if status.is_deleted() { Color::Disabled } else { Color::Muted };

        let id = ElementId::Name(format!("entry_{display_name}_{ix}").into());
        let checkbox_wrapper_id = ElementId::Name(format!("entry_{display_name}_{ix}_checkbox_wrapper").into());
        let checkbox_id = ElementId::Name(format!("entry_{display_name}_{ix}_checkbox").into());

        let stage_status = GitPanel::stage_status_for_entry(entry, repo);
        let stage_intent = self.stage_intent_for_entry_index(ix);
        let resolved_conflict = self.is_resolved_conflict(ix, cx);
        let toggle_state = stage_intent.checkbox_state(|| {
            if self.show_placeholders && !self.has_staged_changes() && !entry.status.is_created() {
                ToggleState::Selected
            } else {
                match stage_status {
                    StageStatus::Staged => ToggleState::Selected,
                    StageStatus::Unstaged => ToggleState::Unselected,
                    StageStatus::PartiallyStaged => ToggleState::Indeterminate,
                }
            }
        });

        let handle = cx.weak_entity();

        let selected_bg_alpha = 0.08;
        let marked_bg_alpha = 0.12;
        let state_opacity_step = 0.04;

        let info_color = cx.theme().status().info;

        let base_bg = match (selected, marked) {
            (true, true) => info_color.alpha(selected_bg_alpha + marked_bg_alpha),
            (true, false) => info_color.alpha(selected_bg_alpha),
            (false, true) => info_color.alpha(marked_bg_alpha),
            _ => cx.theme().colors().ghost_element_background,
        };

        let (hover_bg, active_bg) = if selected {
            (
                info_color.alpha(selected_bg_alpha + state_opacity_step),
                info_color.alpha(selected_bg_alpha + state_opacity_step * 2.0),
            )
        } else {
            (cx.theme().colors().ghost_element_hover, cx.theme().colors().ghost_element_active)
        };

        let name_row = h_flex().min_w_0().flex_1().gap_1().child(git_status_icon(status)).map(|this| {
            if tree_view {
                this.pl(px(depth as f32 * TREE_INDENT)).child(
                    self.entry_label(display_name, label_color)
                        .when(status.is_deleted(), Label::strikethrough)
                        .truncate(),
                )
            } else {
                this.child(self.path_formatted(
                    entry.parent_dir(),
                    path_color,
                    display_name,
                    label_color,
                    status.is_deleted(),
                ))
            }
        });

        let id_for_diff_stat = id.clone();

        h_flex()
            .id(id)
            .h(self.list_item_height())
            .w_full()
            .pl_2p5()
            .pr_1()
            .gap_1p5()
            .border_1()
            .border_r_2()
            .when(selected && self.focus_handle.is_focused(window), |el| {
                el.border_color(cx.theme().colors().panel_focused_border)
            })
            .bg(base_bg)
            .hover(|s| s.bg(hover_bg))
            .active(|s| s.bg(active_bg))
            // Shrinks the name instead of letting it push the stats and checkbox out.
            .child(h_flex().min_w_0().flex_1().overflow_hidden().child(name_row))
            .when_some(entry.diff_stat, move |this, stat| {
                let id = format!("diff-stat-{id_for_diff_stat}");
                this.child(ui::DiffStat::new(id, stat.added as usize, stat.deleted as usize))
            })
            .child(
                div().id(checkbox_wrapper_id).flex_none().occlude().cursor_pointer().child(
                    Checkbox::new(checkbox_id, toggle_state)
                        .fill()
                        .elevation(ElevationIndex::Surface)
                        .disabled(resolved_conflict)
                        .on_click_ext({
                            let entry = entry.clone();
                            let this = cx.weak_entity();
                            move |_, click, window, cx| {
                                this.update(cx, |this, cx| {
                                    if resolved_conflict {
                                        return;
                                    }
                                    if click.modifiers().shift {
                                        this.stage_bulk(ix, stage_intent != StageIntent::Unstage, cx);
                                    } else {
                                        let list_entry = if GitPanelViewOptions::get(cx).tree_view {
                                            GitListEntry::TreeStatus(GitTreeStatusEntry { entry: entry.clone(), depth })
                                        } else {
                                            GitListEntry::Status(entry.clone())
                                        };
                                        this.toggle_staged_for_entry(&list_entry, stage_intent, window, cx);
                                    }
                                    cx.stop_propagation();
                                })
                                .ok();
                            }
                        })
                        .tooltip(move |_window, cx| {
                            if resolved_conflict {
                                Tooltip::simple("Conflict marked as resolved", cx)
                            } else {
                                let action = stage_intent.label(|| stage_status);
                                Tooltip::for_action(action, &ToggleStaged, cx)
                            }
                        }),
                ),
            )
            .on_click({
                cx.listener(move |this, event: &ClickEvent, window, cx| {
                    if event.modifiers().shift {
                        let anchor_ix = this.selected_entry.unwrap_or(ix);
                        this.apply_range_gesture(anchor_ix, ix);
                        this.selected_entry = Some(ix);
                        cx.notify();
                    } else if event.modifiers().secondary() {
                        this.toggle_mark(ix, cx);
                    } else {
                        this.clear_marks_and_select(ix, cx);
                        this.open_selected_entry_on_click(event.click_count() > 1, window, cx);
                    }
                })
            })
            .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
                // why isn't this happening automatically? we are passing MouseButton::Right to `on_mouse_down`?
                if event.button != MouseButton::Right {
                    return;
                }

                let Some(this) = handle.upgrade() else {
                    return;
                };
                this.update(cx, |this, cx| {
                    this.deploy_entry_context_menu(event.position, ix, window, cx);
                });
                cx.stop_propagation();
            })
            .into_any_element()
    }

    fn render_directory_entry(
        &self,
        ix: usize,
        entry: &GitTreeDirEntry,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selected = self.selected_entry == Some(ix);
        let marked = self.marked_directories.contains(&entry.key);
        let label_color = Color::Muted;
        let id = ElementId::Name(format!("dir_{}_{}", entry.name, ix).into());
        let checkbox_id = ElementId::Name(format!("dir_checkbox_{}_{}", entry.name, ix).into());
        let checkbox_wrapper_id = ElementId::Name(format!("dir_checkbox_wrapper_{}_{}", entry.name, ix).into());

        let selected_bg_alpha = 0.08;
        let marked_bg_alpha = 0.12;
        let state_opacity_step = 0.04;

        let info_color = cx.theme().status().info;
        let colors = cx.theme().colors();

        let base_bg = match (selected, marked) {
            (true, true) => info_color.alpha(selected_bg_alpha + marked_bg_alpha),
            (true, false) => info_color.alpha(selected_bg_alpha),
            (false, true) => info_color.alpha(marked_bg_alpha),
            _ => colors.ghost_element_background,
        };

        let (hover_bg, active_bg) = if selected {
            (
                info_color.alpha(selected_bg_alpha + state_opacity_step),
                info_color.alpha(selected_bg_alpha + state_opacity_step * 2.0),
            )
        } else {
            (colors.ghost_element_hover, colors.ghost_element_active)
        };

        let folder_icon = FileIcons::get_folder_icon(entry.expanded, entry.key.path.as_std_path(), cx);
        let fallback_folder_icon = if entry.expanded { IconName::FolderOpen } else { IconName::Folder };

        let stage_status = if let Some(repo) = &self.active_repository {
            self.stage_status_for_directory(entry, repo.read(cx))
        } else {
            log::error!("Won't have entries to render without an active repository in Git Panel");
            StageStatus::PartiallyStaged
        };

        let stage_intent = StageIntent::for_section(entry.key.section);
        let resolved_conflict = self.is_resolved_conflict(ix, cx);
        let toggle_state = stage_intent.checkbox_state(|| match stage_status {
            StageStatus::Staged => ToggleState::Selected,
            StageStatus::Unstaged => ToggleState::Unselected,
            StageStatus::PartiallyStaged => ToggleState::Indeterminate,
        });

        let name_row = h_flex()
            .min_w_0()
            .gap_1()
            .pl(px(entry.depth as f32 * TREE_INDENT))
            .child(h_flex().flex_none().gap_0p5().children({
                let render_indicator = |themed: Option<SharedString>, fallback: IconName| {
                    themed
                        .map_or_else(|| Icon::new(fallback), Icon::from_path)
                        .size(IconSize::Small)
                        .color(Color::Muted)
                };

                [render_indicator(folder_icon, fallback_folder_icon)]
            }))
            .child(self.entry_label(entry.name.clone(), label_color).truncate());
        h_flex()
            .id(id)
            .h(self.list_item_height())
            .min_w_0()
            .w_full()
            .pl_2p5()
            .pr_1()
            .gap_1p5()
            .justify_between()
            .border_1()
            .border_r_2()
            .when(selected && self.focus_handle.is_focused(window), |el| {
                el.border_color(cx.theme().colors().panel_focused_border)
            })
            .bg(base_bg)
            .hover(|s| s.bg(hover_bg))
            .active(|s| s.bg(active_bg))
            .child(name_row)
            .child(
                div().id(checkbox_wrapper_id).flex_none().occlude().cursor_pointer().child(
                    Checkbox::new(checkbox_id, toggle_state)
                        .disabled(resolved_conflict)
                        .fill()
                        .elevation(ElevationIndex::Surface)
                        .on_click({
                            let entry = entry.clone();
                            let this = cx.weak_entity();
                            move |_, window, cx| {
                                this.update(cx, |this, cx| {
                                    if resolved_conflict {
                                        return;
                                    }
                                    this.toggle_staged_for_entry(
                                        &GitListEntry::Directory(entry.clone()),
                                        stage_intent,
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                })
                                .ok();
                            }
                        })
                        .tooltip(move |_window, cx| {
                            if resolved_conflict {
                                Tooltip::simple("Conflicts marked as resolved", cx)
                            } else {
                                let action = stage_intent.label(|| stage_status);
                                Tooltip::simple(format!("{action} Folder"), cx)
                            }
                        }),
                ),
            )
            .on_click({
                let key = entry.key.clone();
                cx.listener(move |this, event: &ClickEvent, window, cx| {
                    if event.modifiers().shift {
                        let anchor_ix = this.selected_entry.unwrap_or(ix);
                        this.apply_range_gesture(anchor_ix, ix);
                        this.selected_entry = Some(ix);
                        cx.notify();
                    } else if event.modifiers().secondary() {
                        this.toggle_mark(ix, cx);
                    } else {
                        this.clear_marks();
                        this.selected_entry = Some(ix);
                        this.toggle_directory(&key, window, cx);
                    }
                })
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.deploy_entry_context_menu(event.position, ix, window, cx);
                }),
            )
            .into_any_element()
    }

    fn path_formatted(
        &self,
        directory: Option<String>,
        path_color: Color,
        file_name: String,
        label_color: Color,
        strikethrough: bool,
    ) -> Div {
        let file_name = format!("{file_name} ");

        h_flex()
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .flex_none()
                    .child(self.entry_label(file_name, label_color).when(strikethrough, Label::strikethrough)),
            )
            .when_some(directory, |this, dir| {
                this.child(self.entry_label(dir, path_color).truncate_start().when(strikethrough, Label::strikethrough))
            })
    }
}
