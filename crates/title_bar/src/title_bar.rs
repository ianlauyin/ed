mod application_menu;
mod branch_status_item;
mod title_bar_settings;

use std::path::Path;

use ed_actions::OpenRemote;
use gpui::AnyElement;
use gpui::App;
use gpui::Context;
use gpui::Entity;
use gpui::Focusable;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Render;
use gpui::Styled;
use gpui::Subscription;
use gpui::WeakEntity;
use gpui::Window;
use gpui::actions;
use project::Project;
use project::git_store::GitStoreEvent;
use project::linked_worktree_short_name;
use project::repo_identity_path;
use project::repo_identity_path_if_local;
use project::trusted_worktrees::TrustedWorktrees;
use remote::RemoteConnectionOptions;
use settings::Settings as _;
use theme::ActiveTheme;
use title_bar_settings::TitleBarSettings;
use ui::ButtonLike;
use ui::IconWithIndicator;
use ui::Indicator;
use ui::PopoverMenu;
use ui::TintColor;
use ui::Tooltip;
use ui::prelude::*;
use util::ResultExt;
use workspace::StatusItemModes;
use workspace::ToggleWorktreeSecurity;
use workspace::Workspace;

use crate::application_menu::ApplicationMenu;
use crate::branch_status_item::BranchStatusItem;

const MAX_PROJECT_NAME_LENGTH: usize = 40;

fn linked_worktree_name_anchor<'a>(
    main_worktree_path: Option<&'a Path>,
    repository_identity_path: Option<&'a Path>,
    is_linked_worktree: bool,
) -> Option<&'a Path> {
    main_worktree_path.or_else(|| is_linked_worktree.then_some(repository_identity_path).flatten())
}

actions!(
    title_bar,
    [
        /// Toggles the project menu dropdown.
        ToggleProjectMenu,
        /// Switches to a different git branch.
        SwitchBranch,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };
        let item = cx.new(|cx| TitleBar::new(workspace, window, cx));
        workspace.set_titlebar_item(item.into(), window, cx);
        let branch_item = cx.new(|cx| BranchStatusItem::new(workspace, cx));
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.prepend_left_item_in(branch_item, StatusItemModes::All, window, cx);
        });
    })
    .detach();
}

/// The project's name and alerts (restricted mode, remote host), shown in the
/// window header above the sidebar or at the start of the top-left tab bar.
pub struct TitleBar {
    project: Entity<Project>,

    workspace: WeakEntity<Workspace>,
    application_menu: Option<Entity<ApplicationMenu>>,
    _subscriptions: Vec<Subscription>,
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_project_items = TitleBarSettings::get_global(cx).show_project_items
            && !self.application_menu.as_ref().is_some_and(|menu| menu.read(cx).all_menus_shown());
        let project_name = show_project_items.then(|| ActiveRepository::of(&self.project, cx).project_name);

        h_flex()
            .h_full()
            .gap_0p5()
            .children(self.application_menu.clone())
            .children(self.render_restricted_mode(cx))
            .when_some(project_name, |title_bar, project_name| {
                title_bar.children(self.render_project_host(cx)).child(self.render_project_name(
                    project_name,
                    window,
                    cx,
                ))
            })
    }
}

/// The repository the window shows: the one owning the project's active
/// repository's worktree, else the first visible worktree's.
struct ActiveRepository {
    project_name: Option<SharedString>,
    repository: Option<Entity<project::git_store::Repository>>,
    linked_worktree_name: Option<SharedString>,
}

impl ActiveRepository {
    fn of(project: &Entity<Project>, cx: &App) -> ActiveRepository {
        let mut active = ActiveRepository { project_name: None, repository: None, linked_worktree_name: None };
        let Some(worktree) = effective_active_worktree(project, cx) else {
            return active;
        };
        active.repository = repository_for_worktree(project, &worktree, cx);
        let worktree_abs_path = worktree.read(cx).abs_path();
        active.project_name =
            worktree.read(cx).root_name().file_name().map(|name| SharedString::from(name.to_string()));
        let Some(repo) = &active.repository else {
            return active;
        };
        let repo = repo.read(cx);
        let identity = repo_identity_path(&repo.common_dir_abs_path, repo.path_style);
        let identity_fallback = repo_identity_path_if_local(&repo.common_dir_abs_path, repo.path_style);
        active.linked_worktree_name =
            linked_worktree_name_anchor(repo.main_worktree_abs_path(), identity_fallback, repo.is_linked_worktree())
                .and_then(|name_anchor_path| {
                    linked_worktree_short_name(name_anchor_path, repo.work_directory_abs_path.as_ref(), repo.path_style)
                })
                .or_else(|| repo.is_linked_worktree().then_some(active.project_name.clone()).flatten());

        let display_name = if identity.extension() == Some(std::ffi::OsStr::new("git")) {
            identity.file_stem()
        } else {
            repo.path_style.file_name(identity)
        };

        if let Some(repo_name) = display_name.and_then(|n| n.to_str()) {
            let name = if visible_worktrees_in_repository(project, repo, cx) == 1
                && let Ok(relative) = worktree_abs_path.strip_prefix(&*repo.work_directory_abs_path)
                && !relative.as_os_str().is_empty()
            {
                format!("{}/{}", repo_name, relative.display())
            } else {
                repo_name.to_string()
            };
            active.project_name = Some(SharedString::from(name));
        }
        active
    }
}

/// Returns the worktree to display in the window header.
/// - Prefer the worktree owning the project's active repository
/// - Fall back to the first visible worktree
fn effective_active_worktree(project: &Entity<Project>, cx: &App) -> Option<Entity<project::Worktree>> {
    let project = project.read(cx);

    if let Some(repo) = project.active_repository(cx) {
        let repo = repo.read(cx);
        let repo_path = &repo.work_directory_abs_path;

        for worktree in project.visible_worktrees(cx) {
            let worktree_path = worktree.read(cx).abs_path();
            if worktree_path == *repo_path || worktree_path.starts_with(repo_path.as_ref()) {
                return Some(worktree);
            }
        }
    }

    project.visible_worktrees(cx).next()
}

fn repository_for_worktree(
    project: &Entity<Project>,
    worktree: &Entity<project::Worktree>,
    cx: &App,
) -> Option<Entity<project::git_store::Repository>> {
    let project = project.read(cx);
    let git_store = project.git_store().read(cx);
    let worktree_path = worktree.read(cx).abs_path();

    git_store
        .repositories()
        .values()
        .filter(|repo| {
            let repo_path = &repo.read(cx).work_directory_abs_path;
            worktree_path == *repo_path || worktree_path.starts_with(repo_path.as_ref())
        })
        .max_by_key(|repo| repo.read(cx).work_directory_abs_path.as_os_str().len())
        .cloned()
}

fn visible_worktrees_in_repository(
    project: &Entity<Project>,
    repository: &project::git_store::Repository,
    cx: &App,
) -> usize {
    let repo_path = &repository.work_directory_abs_path;
    project
        .read(cx)
        .visible_worktrees(cx)
        .filter(|worktree| {
            let worktree_path = worktree.read(cx).abs_path();
            worktree_path == *repo_path || worktree_path.starts_with(repo_path.as_ref())
        })
        .count()
}

impl TitleBar {
    pub fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project = workspace.project().clone();
        let git_store = project.read(cx).git_store().clone();

        let application_menu = if option_env!("ED_USE_CROSS_PLATFORM_MENU").is_some() {
            Some(cx.new(|cx| ApplicationMenu::new(window, cx)))
        } else {
            None
        };

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&git_store, move |_, _, event, cx| match event {
            GitStoreEvent::ActiveRepositoryChanged(_) | GitStoreEvent::RepositoryUpdated(_, _, true) => {
                cx.notify();
            }
            _ => {}
        }));
        subscriptions.push(cx.observe(&project, |_, _, cx| cx.notify()));
        if let Some(trusted_worktrees) = TrustedWorktrees::try_get_global(cx) {
            subscriptions.push(cx.subscribe(&trusted_worktrees, |_, _, _, cx| {
                cx.notify();
            }));
        }

        Self { application_menu, workspace: workspace.weak_handle(), project, _subscriptions: subscriptions }
    }

    fn worktree_count(&self, cx: &App) -> usize {
        self.project.read(cx).visible_worktrees(cx).count()
    }

    fn render_remote_project_connection(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let workspace = self.workspace.clone();

        let options = self.project.read(cx).remote_connection_options(cx)?;
        let host: SharedString = options.display_name().into();

        let (nickname, tooltip_title, icon) = match options {
            RemoteConnectionOptions::Ssh(options) => {
                (options.nickname.map(|nick| nick.into()), "Remote Project", IconName::Server)
            }
        };

        let nickname = nickname.unwrap_or_else(|| host.clone());

        let (indicator_color, meta) = match self.project.read(cx).remote_connection_state(cx)? {
            remote::ConnectionState::Connecting => (Color::Info, format!("Connecting to: {host}")),
            remote::ConnectionState::Connected => (Color::Success, format!("Connected to: {host}")),
            remote::ConnectionState::HeartbeatMissed => {
                (Color::Warning, format!("Connection attempt to {host} missed. Retrying..."))
            }
            remote::ConnectionState::Reconnecting => {
                (Color::Warning, format!("Lost connection to {host}. Reconnecting..."))
            }
            remote::ConnectionState::Disconnected => (Color::Error, format!("Disconnected from {host}")),
        };

        let icon_color = match self.project.read(cx).remote_connection_state(cx)? {
            remote::ConnectionState::Connecting => Color::Info,
            remote::ConnectionState::Connected => Color::Default,
            remote::ConnectionState::HeartbeatMissed => Color::Warning,
            remote::ConnectionState::Reconnecting => Color::Warning,
            remote::ConnectionState::Disconnected => Color::Error,
        };

        let meta = SharedString::from(meta);

        Some(
            PopoverMenu::new("remote-project-menu")
                .menu(move |window, cx| {
                    workspace.upgrade()?;
                    Some(recent_projects::RemoteServerProjects::popover(workspace.clone(), None, window, cx))
                })
                .trigger_with_tooltip(
                    ButtonLike::new("remote_project").selected_style(ButtonStyle::Tinted(TintColor::Accent)).child(
                        h_flex()
                            .gap_2()
                            .max_w_32()
                            .child(
                                IconWithIndicator::new(
                                    Icon::new(icon).size(IconSize::Small).color(icon_color),
                                    Some(Indicator::dot().color(indicator_color)),
                                )
                                .indicator_border_color(Some(cx.theme().colors().title_bar_background))
                                .into_any_element(),
                            )
                            .child(Label::new(nickname).size(LabelSize::Small).truncate()),
                    ),
                    move |_window, cx| {
                        Tooltip::with_meta(tooltip_title, Some(&OpenRemote::default()), meta.clone(), cx)
                    },
                )
                .anchor(gpui::Anchor::TopLeft)
                .into_any_element(),
        )
    }

    pub fn render_restricted_mode(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let has_restricted_worktrees =
            TrustedWorktrees::has_restricted_worktrees(&self.project.read(cx).worktree_store(), cx);
        if !has_restricted_worktrees {
            return None;
        }

        let button = Button::new("restricted_mode_trigger", "Restricted Mode")
            .style(ButtonStyle::Tinted(TintColor::Warning))
            .label_size(LabelSize::Small)
            .color(Color::Warning)
            .start_icon(Icon::new(IconName::Warning).size(IconSize::Small).color(Color::Warning))
            .tooltip(|_, cx| {
                Tooltip::with_meta(
                    "You're in Restricted Mode",
                    Some(&ToggleWorktreeSecurity),
                    "Mark this project as trusted and unlock all features",
                    cx,
                )
            })
            .on_click({
                cx.listener(move |this, _, window, cx| {
                    this.workspace
                        .update(cx, |workspace, cx| workspace.show_worktree_trust_security_modal(true, window, cx))
                        .log_err();
                })
            });

        Some(button.into_any_element())
    }

    pub fn render_project_host(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.project.read(cx).is_via_remote_server() {
            return self.render_remote_project_connection(cx);
        }

        if self.project.read(cx).is_disconnected(cx) {
            return Some(
                Button::new("disconnected", "Disconnected")
                    .disabled(true)
                    .color(Color::Disabled)
                    .label_size(LabelSize::Small)
                    .into_any_element(),
            );
        }

        None
    }

    fn render_project_name(&self, name: Option<SharedString>, _: &mut Window, cx: &Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.clone();

        let is_project_selected = name.is_some();

        let display_name = if let Some(ref name) = name {
            util::truncate_and_trailoff(name, MAX_PROJECT_NAME_LENGTH)
        } else {
            "Open Recent Project".to_string()
        };

        let focus_handle =
            workspace.upgrade().map(|w| w.read(cx).focus_handle(cx)).unwrap_or_else(|| cx.focus_handle());

        PopoverMenu::new("recent-projects-menu")
            .menu(move |window, cx| {
                Some(recent_projects::RecentProjects::popover(
                    workspace.clone(),
                    None,
                    focus_handle.clone(),
                    window,
                    cx,
                ))
            })
            .trigger_with_tooltip(
                Button::new("project_name_trigger", display_name)
                    .label_size(LabelSize::Small)
                    .tab_index(0isize)
                    .when(self.worktree_count(cx) > 1, |this| {
                        this.end_icon(Icon::new(IconName::ChevronDown).size(IconSize::XSmall).color(Color::Muted))
                    })
                    .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                    .when(!is_project_selected, |s| s.color(Color::Muted)),
                move |_window, cx| Tooltip::for_action("Recent Projects", &ed_actions::OpenRecent::default(), cx),
            )
            .anchor(gpui::Anchor::TopLeft)
            .into_any_element()
    }
}
