use crate::commit_view::CommitView;
use anyhow::Result;
use editor::hover_markdown_style;
use git::blame::BlameEntry;
use git::commit::ParsedCommitMessage;
use git::repository::CommitSummary;
use git_ui_core::notifications::show_error_toast;
use gpui::{
    App, Element, Entity, MouseButton, ParentElement, Render, ScrollHandle,
    StatefulInteractiveElement, Task, WeakEntity, Window, prelude::*,
};
use markdown::{Markdown, MarkdownElement};
use notifications::status_toast::StatusToast;
use project::git_store::{Repository, UnshallowState};
use settings::Settings;
use theme_settings::ThemeSettings;
use time::{OffsetDateTime, UtcOffset};
use ui::{Chip, CopyButton, Divider, Tooltip, prelude::*, tooltip_container};
use workspace::Workspace;

#[derive(Clone, Debug)]
pub struct CommitDetails {
    pub sha: SharedString,
    pub author_name: SharedString,
    pub author_email: SharedString,
    pub commit_time: OffsetDateTime,
    pub message: Option<ParsedCommitMessage>,
    pub tag_names: Vec<SharedString>,
    pub boundary: bool,
}

const MAX_COMMIT_TOOLTIP_TAG_CHIPS: usize = 2;

pub(crate) fn commit_tag_chips(tag_names: &[SharedString]) -> Option<impl IntoElement> {
    if tag_names.is_empty() {
        return None;
    }

    let (visible_tags, hidden_tags) =
        tag_names.split_at(tag_names.len().min(MAX_COMMIT_TOOLTIP_TAG_CHIPS));

    Some(
        h_flex().max_w(relative(0.6)).gap_1().child(
            h_flex()
                .gap_1()
                .min_w_0()
                .children(
                    visible_tags
                        .iter()
                        .map(|tag_name| Chip::new(tag_name.clone()).truncate()),
                )
                .when(!hidden_tags.is_empty(), |this| {
                    let hidden_tags = hidden_tags.to_vec();
                    this.child(Chip::new(format!("+{}", hidden_tags.len())).tooltip(
                        Tooltip::element(move |_window, cx| {
                            v_flex()
                                .gap_1()
                                .children(itertools::Itertools::intersperse_with(
                                    hidden_tags.iter().map(|tag_name| {
                                        Label::new(tag_name.clone())
                                            .size(LabelSize::Small)
                                            .buffer_font(cx)
                                            .into_any_element()
                                    }),
                                    || Divider::horizontal().into_any_element(),
                                ))
                                .into_any_element()
                        }),
                    ))
                })
                .child(Divider::vertical()),
        ),
    )
}

pub struct CommitTooltip {
    commit: CommitDetails,
    scroll_handle: ScrollHandle,
    markdown: Entity<Markdown>,
    repository: Entity<Repository>,
    workspace: WeakEntity<Workspace>,
}

impl CommitTooltip {
    pub fn blame_entry(
        blame: &BlameEntry,
        details: Option<ParsedCommitMessage>,
        tag_names: Vec<SharedString>,
        repository: Entity<Repository>,
        workspace: WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let commit_time = blame
            .committer_time
            .and_then(|t| OffsetDateTime::from_unix_timestamp(t).ok())
            .unwrap_or(OffsetDateTime::now_utc());

        Self::new(
            CommitDetails {
                sha: blame.sha.to_string().into(),
                commit_time,
                author_name: blame
                    .author
                    .clone()
                    .unwrap_or("<no name>".to_string())
                    .into(),
                author_email: blame.author_mail.clone().unwrap_or("".to_string()).into(),
                message: details,
                tag_names,
                boundary: blame.boundary,
            },
            repository,
            workspace,
            cx,
        )
    }

    pub fn new(
        commit: CommitDetails,
        repository: Entity<Repository>,
        workspace: WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let markdown = cx.new(|cx| {
            Markdown::new(
                commit
                    .message
                    .as_ref()
                    .map(|message| message.message.clone())
                    .unwrap_or_default(),
                None,
                None,
                cx,
            )
        });
        Self {
            commit,
            repository,
            workspace,
            scroll_handle: ScrollHandle::new(),
            markdown,
        }
    }
}

impl Render for CommitTooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let author = self.commit.author_name.clone();

        let author_email = self.commit.author_email.clone();

        let short_commit_id = self
            .commit
            .sha
            .get(0..git::SHORT_SHA_LENGTH)
            .map(|sha| sha.to_string().into())
            .unwrap_or_else(|| self.commit.sha.clone());
        let full_sha = self.commit.sha.to_string();
        let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        let absolute_timestamp = time_format::format_localized_timestamp(
            self.commit.commit_time,
            OffsetDateTime::now_utc(),
            local_offset,
            time_format::TimestampFormat::MediumAbsolute,
        );
        let markdown_style = {
            let style = hover_markdown_style(window, cx);
            style
        };

        let message = self
            .commit
            .message
            .as_ref()
            .map(|_| {
                MarkdownElement::new(self.markdown.clone(), markdown_style)
                    .scroll_handle(self.scroll_handle.clone())
                    .into_any()
            })
            .unwrap_or("<no commit message>".into_any());

        let tag_names = self.commit.tag_names.clone();

        let ui_font_size = ThemeSettings::get_global(cx).ui_font_size(cx);
        let message_max_height = window.line_height() * 12 + (ui_font_size / 0.4);
        let repo = self.repository.clone();
        let workspace = self.workspace.clone();
        let commit_summary = CommitSummary {
            sha: self.commit.sha.clone(),
            subject: self
                .commit
                .message
                .as_ref()
                .map_or(Default::default(), |message| {
                    message
                        .message
                        .split('\n')
                        .next()
                        .unwrap()
                        .trim_end()
                        .to_string()
                        .into()
                }),
            commit_timestamp: self.commit.commit_time.unix_timestamp(),
            author_name: self.commit.author_name.clone(),
            has_parent: false,
        };
        let boundary_notice = self
            .commit
            .boundary
            .then(|| {
                shallow_boundary_notice(self.repository.clone(), self.workspace.clone(), window, cx)
            })
            .flatten();

        tooltip_container(cx, move |this, cx| {
            this.occlude()
                .on_mouse_move(|_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    v_flex()
                        .w(gpui::rems(30.))
                        .child(
                            h_flex()
                                .pb_1()
                                .gap_2()
                                .overflow_x_hidden()
                                .flex_wrap()
                                .child(author)
                                .when(!author_email.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .text_color(cx.theme().colors().text_muted)
                                            .child(author_email),
                                    )
                                })
                                .border_b_1()
                                .border_color(cx.theme().colors().border_variant),
                        )
                        .children(boundary_notice)
                        .child(
                            div()
                                .id("inline-blame-commit-message")
                                .track_scroll(&self.scroll_handle)
                                .py_1p5()
                                .max_h(message_max_height)
                                .overflow_y_scroll()
                                .child(message),
                        )
                        .child(
                            h_flex()
                                .text_color(cx.theme().colors().text_muted)
                                .w_full()
                                .justify_between()
                                .pt_1()
                                .gap_1()
                                .flex_wrap()
                                .border_t_1()
                                .border_color(cx.theme().colors().border_variant)
                                .child(absolute_timestamp)
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .min_w_0()
                                        .children(commit_tag_chips(&tag_names))
                                        .child(
                                            Button::new(
                                                "commit-sha-button",
                                                short_commit_id.clone(),
                                            )
                                            .color(Color::Muted)
                                            .start_icon(
                                                Icon::new(IconName::FileGit)
                                                    .size(IconSize::Small)
                                                    .color(Color::Muted),
                                            )
                                            .on_click(
                                                move |_, window, cx| {
                                                    CommitView::open(
                                                        commit_summary.sha.to_string(),
                                                        repo.downgrade(),
                                                        workspace.clone(),
                                                        None,
                                                        None,
                                                        window,
                                                        cx,
                                                    );
                                                    cx.stop_propagation();
                                                },
                                            ),
                                        )
                                        .child(Divider::vertical())
                                        .child(
                                            CopyButton::new("copy-commit-sha", full_sha)
                                                .tooltip_label("Copy SHA"),
                                        ),
                                ),
                        ),
                )
        })
    }
}

fn blame_entry_timestamp(blame_entry: &BlameEntry, format: time_format::TimestampFormat) -> String {
    match blame_entry.author_offset_date_time() {
        Ok(timestamp) => {
            let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
            time_format::format_localized_timestamp(
                timestamp,
                time::OffsetDateTime::now_utc(),
                local_offset,
                format,
            )
        }
        Err(_) => "Error parsing date".to_string(),
    }
}

pub fn blame_entry_relative_timestamp(blame_entry: &BlameEntry) -> String {
    blame_entry_timestamp(blame_entry, time_format::TimestampFormat::Relative)
}

pub(crate) fn shallow_boundary_notice(
    repository: Entity<Repository>,
    workspace: WeakEntity<Workspace>,
    window: &Window,
    cx: &App,
) -> Option<impl IntoElement + use<>> {
    let unshallow_state = repository.read(cx).unshallow_state();
    if unshallow_state == UnshallowState::Unshallowed {
        return None;
    }
    let in_flight = unshallow_state == UnshallowState::InProgress;
    let icon_width = rems(1.).to_pixels(window.rem_size());
    let can_fetch = workspace
        .read_with(cx, |workspace, cx| {
            !workspace.project().read(cx).is_read_only(cx)
        })
        .unwrap_or(false);
    Some(
        v_flex()
            .py_1()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        h_flex().w(icon_width).justify_center().child(
                            Icon::new(IconName::Warning)
                                .size(IconSize::Small)
                                .color(Color::Warning),
                        ),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(
                                "Shallow clone boundary: earlier history is missing, so these lines may come from an older commit.",
                            )
                            .size(LabelSize::Small)
                            .line_height_style(LineHeightStyle::UiLabel),
                        ),
                    ),
            )
            .when(can_fetch, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(div().w(icon_width))
                        .child(
                            Button::new(
                                "fetch-unshallow",
                                if in_flight {
                                    "Fetching…"
                                } else {
                                    "Fetch Missing History"
                                },
                            )
                            .style(ButtonStyle::Outlined)
                            .label_size(LabelSize::Small)
                            .disabled(in_flight)
                            .tooltip(Tooltip::text(
                                "Run `git fetch --unshallow` to download the full history",
                            ))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                fetch_unshallow(
                                    repository.clone(),
                                    workspace.clone(),
                                    window,
                                    cx,
                                )
                                .detach_and_log_err(cx);
                            }),
                        ),
                )
            }),
    )
}

pub(crate) fn fetch_unshallow(
    repository: Entity<Repository>,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<()>> {
    if repository.read(cx).unshallow_state() != UnshallowState::Idle {
        return Task::ready(Ok(()));
    }
    let fetch = repository.update(cx, |repository, cx| repository.fetch_unshallow(cx));
    window.refresh();
    window.spawn(cx, async move |cx| {
        let result = match fetch.await {
            Ok(result) => result,
            Err(canceled) => Err(anyhow::Error::from(canceled)),
        };
        cx.update(|window, cx| {
            window.refresh();
            let Some(workspace) = workspace.upgrade() else {
                return Ok(());
            };
            match result {
                Ok(_) => {
                    workspace.update(cx, |workspace, cx| {
                        let toast = StatusToast::new(
                            "Fetched the missing commit history",
                            cx,
                            |this, _| {
                                this.icon(
                                    Icon::new(IconName::GitBranch)
                                        .size(IconSize::Small)
                                        .color(Color::Muted),
                                )
                            },
                        );
                        workspace.toggle_status_toast(toast, cx);
                    });
                    Ok(())
                }
                Err(error) => {
                    show_error_toast(workspace, "fetch --unshallow", error, cx);
                    Err(anyhow::anyhow!("git fetch --unshallow failed"))
                }
            }
        })?
    })
}
