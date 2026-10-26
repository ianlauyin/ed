use gpui::ClickEvent;
use gpui::DismissEvent;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::Render;
use gpui::WeakEntity;
use project::project_settings::ProjectSettings;
use remote::RemoteConnectionOptions;
use settings::Settings;
use ui::ElevationIndex;
use ui::Modal;
use ui::ModalFooter;
use ui::ModalHeader;
use ui::Section;
use ui::prelude::*;
use workspace::ModalView;
use workspace::MultiWorkspace;
use workspace::OpenOptions;
use workspace::Workspace;
use workspace::notifications::DetachAndPromptErr;

use crate::open_remote_project;

pub struct DisconnectedOverlay {
    workspace: WeakEntity<Workspace>,
    connection_options: RemoteConnectionOptions,
    server_not_running: bool,
    focus_handle: FocusHandle,
    finished: bool,
}

impl EventEmitter<DismissEvent> for DisconnectedOverlay {}
impl Focusable for DisconnectedOverlay {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
impl ModalView for DisconnectedOverlay {
    fn on_before_dismiss(&mut self, _window: &mut Window, _: &mut Context<Self>) -> workspace::DismissDecision {
        workspace::DismissDecision::Dismiss(self.finished)
    }
    fn fade_out_background(&self) -> bool {
        true
    }
}

impl DisconnectedOverlay {
    pub fn register(workspace: &mut Workspace, window: Option<&mut Window>, cx: &mut Context<Workspace>) {
        let Some(window) = window else {
            return;
        };
        cx.subscribe_in(workspace.project(), window, |workspace, project, event, window, cx| {
            if !matches!(event, project::Event::DisconnectedFromHost | project::Event::DisconnectedFromRemote { .. }) {
                return;
            }
            let handle = cx.entity().downgrade();

            let Some(connection_options) = project.read(cx).remote_connection_options(cx) else {
                return;
            };
            let server_not_running =
                matches!(event, project::Event::DisconnectedFromRemote { server_not_running: true });

            workspace.toggle_modal(window, cx, |_, cx| DisconnectedOverlay {
                finished: false,
                workspace: handle,
                connection_options,
                server_not_running,
                focus_handle: cx.focus_handle(),
            });
        })
        .detach();
    }

    fn handle_reconnect(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.finished = true;
        cx.emit(DismissEvent);

        self.reconnect_to_remote_project(self.connection_options.clone(), window, cx);
    }

    fn reconnect_to_remote_project(
        &self,
        connection_options: RemoteConnectionOptions,
        window: &Window,
        cx: &Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };

        let Some(window_handle) = window.window_handle().downcast::<MultiWorkspace>() else {
            return;
        };

        let app_state = workspace.read(cx).app_state().clone();
        let paths = workspace.read(cx).root_paths(cx).iter().map(|path| path.to_path_buf()).collect();

        cx.spawn_in(window, async move |_, cx| {
            open_remote_project(
                connection_options,
                paths,
                app_state,
                OpenOptions { requesting_window: Some(window_handle), ..Default::default() },
                cx,
            )
            .await?;
            Ok(())
        })
        .detach_and_prompt_err("Failed to reconnect", window, cx, |_| None);
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.finished = true;
        cx.emit(DismissEvent)
    }
}

impl Render for DisconnectedOverlay {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let autosave = if ProjectSettings::get_global(cx).session.restore_unsaved_buffers {
            "\nUnsaved changes are stored locally."
        } else {
            ""
        };
        let reason = if self.server_not_running { "process exiting unexpectedly" } else { "not responding" };
        let message = format!(
            "Your connection to {} has been lost due to the server {reason}.{autosave}",
            self.connection_options.display_name(),
        );

        div()
            .track_focus(&self.focus_handle(cx))
            .elevation_3(cx)
            .on_action(cx.listener(Self::cancel))
            .occlude()
            .w(rems(24.))
            .max_h(rems(40.))
            .child(
                Modal::new("disconnected", None)
                    .header(
                        ModalHeader::new()
                            .show_dismiss_button(true)
                            .child(Headline::new("Disconnected").size(HeadlineSize::Small)),
                    )
                    .section(Section::new().child(Label::new(message)))
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("close-window", "Close Window")
                                        .style(ButtonStyle::Filled)
                                        .layer(ElevationIndex::ModalSurface)
                                        .on_click(cx.listener(move |_, _, window, _| {
                                            window.remove_window();
                                        })),
                                )
                                .map(|el| {
                                    el.child(
                                        Button::new("reconnect", "Reconnect")
                                            .style(ButtonStyle::Filled)
                                            .layer(ElevationIndex::ModalSurface)
                                            .start_icon(Icon::new(IconName::ArrowCircle))
                                            .on_click(cx.listener(Self::handle_reconnect)),
                                    )
                                }),
                        ),
                    ),
            )
    }
}
