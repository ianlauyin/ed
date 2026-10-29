use editor::Editor;
use gpui::App;
use gpui::Context;
use gpui::Entity;
use gpui::SharedString;
use language::Buffer;
use notifications::status_toast::StatusToast;
use proto::RpcError;
use ui::prelude::*;
use workspace::Workspace;

pub fn open_output(
    operation: impl Into<SharedString>,
    workspace: &mut Workspace,
    output: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let operation = operation.into();

    let plain_text = terminal::strip_ansi_text(output.as_bytes());

    let buffer = cx.new(|cx| Buffer::local(plain_text.as_str(), cx));
    buffer.update(cx, |buffer, cx| {
        buffer.set_capability(language::Capability::ReadOnly, cx);
    });
    let editor = cx.new(|cx| {
        let mut editor = Editor::for_buffer(buffer, None, window, cx);
        editor.buffer().update(cx, |buffer, cx| {
            buffer.set_title(format!("Output from git {operation}"), cx);
        });
        editor.set_read_only(true);
        editor
    });

    workspace.add_item_to_center(Box::new(editor), window, cx);
}

fn is_non_fast_forward_error(error_message: &str) -> bool {
    (error_message.contains("[rejected]")
        && (error_message.contains("(non-fast-forward)") || error_message.contains("(fetch first)")))
        || (error_message.contains("Updates were rejected")
            && (error_message.contains("behind") || error_message.contains("remote")))
}

fn git_error_toast_title(action: &str, message: &str) -> String {
    if action == "push" && is_non_fast_forward_error(message) {
        "Push rejected: fetch and integrate remote changes before pushing again".to_string()
    } else {
        format!("git {action} failed")
    }
}

pub fn show_error_toast(workspace: Entity<Workspace>, action: impl Into<SharedString>, e: anyhow::Error, cx: &mut App) {
    let action = action.into();
    let message = format_git_error_toast_message(&e);
    cx.defer(move |cx| {
        workspace.update(cx, |workspace, cx| {
            let workspace_weak = cx.weak_entity();
            let title = git_error_toast_title(&action, &message);
            let toast = StatusToast::new(title, cx, |this, _cx| {
                this.icon(Icon::new(IconName::XCircle).size(IconSize::Small).color(Color::Error)).action(
                    "View Log",
                    move |window, cx| {
                        let message = message.clone();
                        let action = action.clone();
                        workspace_weak
                            .update(cx, move |workspace, cx| open_output(action, workspace, &message, window, cx))
                            .ok();
                    },
                )
            });
            workspace.toggle_status_toast(toast, cx)
        });
    });
}

fn rpc_error_raw_message_from_chain(error: &anyhow::Error) -> Option<&str> {
    error.chain().find_map(|cause| cause.downcast_ref::<RpcError>().map(RpcError::raw_message))
}

fn format_git_error_toast_message(error: &anyhow::Error) -> String {
    if let Some(message) = rpc_error_raw_message_from_chain(error) {
        message.trim().to_string()
    } else {
        error.to_string().trim().to_string()
    }
}
