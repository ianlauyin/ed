use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::select;
use gpui::App;
use gpui::AsyncApp;
use gpui::DismissEvent;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::ParentElement as _;
use gpui::Render;
use gpui::SharedString;
use gpui::Task;
use gpui::WeakEntity;
use remote::ConnectionIdentifier;
use remote::RemoteClient;
use remote::RemoteConnectionOptions;
use ui::ActiveTheme;
use ui::CommonAnimationExt;
use ui::Context;
use ui::InteractiveElement;
use ui::KeyBinding;
use ui::ListItem;
use ui::prelude::*;
use workspace::DismissDecision;
use workspace::ModalView;
use workspace::Workspace;

pub struct RemoteConnectionPrompt {
    connection_string: SharedString,
    nickname: Option<SharedString>,
    status_message: Option<SharedString>,
    cancellation: Option<oneshot::Sender<()>>,
    focus_handle: FocusHandle,
}

impl Drop for RemoteConnectionPrompt {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancellation.take() {
            log::debug!("cancelling remote connection");
            cancel.send(()).ok();
        }
    }
}

pub struct RemoteConnectionModal {
    pub prompt: Entity<RemoteConnectionPrompt>,
    paths: Vec<PathBuf>,
    finished: bool,
}

impl RemoteConnectionPrompt {
    pub fn new(
        connection_string: String,
        nickname: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            connection_string: connection_string.into(),
            nickname: nickname.map(|nickname| nickname.into()),
            focus_handle: cx.focus_handle(),
            status_message: None,
            cancellation: None,
        }
    }

    pub fn set_cancellation_tx(&mut self, tx: oneshot::Sender<()>) {
        self.cancellation = Some(tx);
    }

    pub fn set_status(&mut self, status: Option<String>, cx: &mut Context<Self>) {
        self.status_message = status.map(|s| s.into());
        cx.notify();
    }
}

impl Render for RemoteConnectionPrompt {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().p_2().size_full().when_some(self.status_message.clone(), |this, status_message| {
            this.child(
                h_flex()
                    .min_w_0()
                    .w_full()
                    .mt_1()
                    .gap_1()
                    .child(
                        Icon::new(IconName::LoadCircle)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                            .with_rotate_animation(2),
                    )
                    .child(
                        Label::new(format!("{}…", status_message))
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .truncate()
                            .flex_1(),
                    ),
            )
        })
    }
}

impl RemoteConnectionModal {
    pub fn new(
        connection_options: &RemoteConnectionOptions,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (connection_string, nickname) = match connection_options {
            RemoteConnectionOptions::Ssh(options) => (options.connection_string(), options.nickname.clone()),
        };
        Self {
            prompt: cx.new(|cx| RemoteConnectionPrompt::new(connection_string, nickname, window, cx)),
            finished: false,
            paths,
        }
    }

    pub fn finished(&mut self, cx: &mut Context<Self>) {
        self.finished = true;
        cx.emit(DismissEvent);
    }

    fn dismiss(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tx) = self.prompt.update(cx, |prompt, _cx| prompt.cancellation.take()) {
            log::debug!("cancelling remote connection");
            tx.send(()).ok();
        }
        self.finished(cx);
    }
}

pub struct SshConnectionHeader {
    pub connection_string: SharedString,
    pub paths: Vec<PathBuf>,
    pub nickname: Option<SharedString>,
}

impl RenderOnce for SshConnectionHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();

        let mut header_color = theme.colors().text;
        header_color.fade_out(0.96);

        let (main_label, meta_label) = if let Some(nickname) = self.nickname {
            (nickname, Some(format!("({})", self.connection_string)))
        } else {
            (self.connection_string, None)
        };

        h_flex()
            .px(DynamicSpacing::Base12.rems())
            .pt(DynamicSpacing::Base08.rems())
            .pb(DynamicSpacing::Base04.rems())
            .rounded_t_sm()
            .w_full()
            .gap_1p5()
            .child(Icon::new(IconName::Server).size(IconSize::Small))
            .child(
                h_flex()
                    .gap_1()
                    .overflow_x_hidden()
                    .child(
                        div()
                            .max_w_96()
                            .overflow_x_hidden()
                            .text_ellipsis()
                            .child(Headline::new(main_label).size(HeadlineSize::XSmall)),
                    )
                    .children(meta_label.map(|label| Label::new(label).color(Color::Muted).size(LabelSize::Small)))
                    .child(div().overflow_x_hidden().text_ellipsis().children(self.paths.into_iter().map(|path| {
                        Label::new(path.to_string_lossy().into_owned()).size(LabelSize::Small).color(Color::Muted)
                    }))),
            )
    }
}

impl Render for RemoteConnectionModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl ui::IntoElement {
        let nickname = self.prompt.read(cx).nickname.clone();
        let connection_string = self.prompt.read(cx).connection_string.clone();

        let theme = cx.theme().clone();
        let body_color = theme.colors().editor_background;

        v_flex()
            .elevation_3(cx)
            .w(rems(34.))
            .border_1()
            .border_color(theme.colors().border)
            .key_context("SshConnectionModal")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::dismiss))
            .child(SshConnectionHeader { paths: self.paths.clone(), connection_string, nickname }.render(window, cx))
            .child(
                div()
                    .w_full()
                    .bg(body_color)
                    .border_y_1()
                    .border_color(theme.colors().border_variant)
                    .child(self.prompt.clone()),
            )
            .child(
                div().w_full().py_1().child(
                    ListItem::new("li-remote-connection-cancel")
                        .inset(true)
                        .spacing(ui::ListItemSpacing::Sparse)
                        .start_slot(Icon::new(IconName::Close).color(Color::Muted))
                        .child(Label::new("Cancel"))
                        .end_slot(
                            KeyBinding::for_action_in(&menu::Cancel, &self.focus_handle(cx), cx)
                                .size(rems_from_px(12_f32)),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.dismiss(&menu::Cancel, window, cx);
                        })),
                ),
            )
    }
}

impl Focusable for RemoteConnectionModal {
    fn focus_handle(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.prompt.read(cx).focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for RemoteConnectionModal {}

impl ModalView for RemoteConnectionModal {
    fn on_before_dismiss(&mut self, _window: &mut Window, _: &mut Context<Self>) -> DismissDecision {
        DismissDecision::Dismiss(self.finished)
    }

    fn fade_out_background(&self) -> bool {
        true
    }
}

#[derive(Clone)]
pub struct RemoteClientDelegate {
    ui: WeakEntity<RemoteConnectionPrompt>,
}

impl RemoteClientDelegate {
    pub const fn new(ui: WeakEntity<RemoteConnectionPrompt>) -> Self {
        Self { ui }
    }
}

impl remote::RemoteClientDelegate for RemoteClientDelegate {
    fn set_status(&self, status: Option<&str>, cx: &mut AsyncApp) {
        self.update_status(status, cx)
    }
}

impl RemoteClientDelegate {
    fn update_status(&self, status: Option<&str>, cx: &AsyncApp) {
        cx.update(|cx| {
            self.ui
                .update(cx, |modal, cx| {
                    modal.set_status(status.map(|s| s.to_string()), cx);
                })
                .ok()
        });
    }
}

/// Shows a [`RemoteConnectionModal`] on the given workspace and establishes
/// a remote connection. This is a convenience wrapper around
/// [`RemoteConnectionModal`] and [`connect`] suitable for use as the
/// `connect_remote` callback in [`MultiWorkspace::find_or_create_workspace`].
///
/// When the global connection pool already has a live connection for the
/// given options, the modal is skipped entirely and the connection is
/// reused silently.
pub fn connect_with_modal(
    workspace: &Entity<Workspace>,
    connection_options: RemoteConnectionOptions,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<Option<Entity<RemoteClient>>>> {
    if remote::has_active_connection(&connection_options, cx) {
        return connect_reusing_pool(connection_options, cx);
    }

    workspace.update(cx, |workspace, cx| {
        workspace.toggle_modal(window, cx, |window, cx| {
            RemoteConnectionModal::new(&connection_options, Vec::new(), window, cx)
        });
        let Some(modal) = workspace.active_modal::<RemoteConnectionModal>(cx) else {
            return Task::ready(Err(anyhow::anyhow!("Failed to open remote connection dialog")));
        };
        let prompt = modal.read(cx).prompt.clone();
        connect(ConnectionIdentifier::setup(), connection_options, prompt, window, cx)
    })
}

/// Dismisses any active [`RemoteConnectionModal`] on the given workspace.
///
/// This should be called after a remote connection attempt completes
/// (success or failure) when the modal was shown on a workspace that may
/// outlive the connection flow — for example, when the modal is shown
/// on a local workspace before switching to a newly-created remote
/// workspace.
pub fn dismiss_connection_modal(workspace: &Entity<Workspace>, cx: &mut gpui::AsyncWindowContext) {
    workspace
        .update_in(cx, |workspace, _window, cx| {
            if let Some(modal) = workspace.active_modal::<RemoteConnectionModal>(cx) {
                modal.update(cx, |modal, cx| modal.finished(cx));
            }
        })
        .ok();
}

/// Creates a [`RemoteClient`] by reusing an existing connection from the
/// global pool. No interactive UI is shown. This should only be called
/// when [`remote::has_active_connection`] returns `true`.
pub fn connect_reusing_pool(
    connection_options: RemoteConnectionOptions,
    cx: &mut App,
) -> Task<Result<Option<Entity<RemoteClient>>>> {
    let delegate: Arc<dyn remote::RemoteClientDelegate> = Arc::new(BackgroundRemoteClientDelegate);

    cx.spawn(async move |cx| {
        let connection = remote::connect(connection_options, delegate.clone(), cx).await?;

        let (_cancel_guard, cancel_rx) = oneshot::channel::<()>();
        cx.update(|cx| RemoteClient::new(ConnectionIdentifier::setup(), connection, cancel_rx, delegate, cx)).await
    })
}

/// Delegate for remote connections that reuse an existing pooled
/// connection.
struct BackgroundRemoteClientDelegate;

impl remote::RemoteClientDelegate for BackgroundRemoteClientDelegate {
    fn set_status(&self, _status: Option<&str>, _cx: &mut AsyncApp) {}
}

pub fn connect(
    unique_identifier: ConnectionIdentifier,
    connection_options: RemoteConnectionOptions,
    ui: Entity<RemoteConnectionPrompt>,
    _window: &mut Window,
    cx: &mut App,
) -> Task<Result<Option<Entity<RemoteClient>>>> {
    let (tx, mut rx) = oneshot::channel();
    ui.update(cx, |ui, _cx| ui.set_cancellation_tx(tx));

    let delegate = Arc::new(RemoteClientDelegate { ui: ui.downgrade() });

    cx.spawn(async move |cx| {
        let connection = remote::connect(connection_options, delegate.clone(), cx);
        let connection = select! {
            _ = rx => return Ok(None),
            result = connection.fuse() => result,
        }?;

        cx.update(|cx| remote::RemoteClient::new(unique_identifier, connection, rx, delegate, cx)).await
    })
}
