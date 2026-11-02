use std::time::Duration;

use editor::Editor;
use editor::MultiBufferOffset;
use gpui::Context;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Render;
use gpui::Styled;
use gpui::Subscription;
use gpui::Task;
use gpui::WeakEntity;
use gpui::Window;
use language::Diagnostic;
use project::project_settings::GoToDiagnosticSeverityFilter;
use project::project_settings::ProjectSettings;
use settings::Settings;
use ui::Button;
use ui::ButtonLike;
use ui::Color;
use ui::Icon;
use ui::IconName;
use ui::Label;
use ui::Tooltip;
use ui::h_flex;
use ui::prelude::*;
use util::ResultExt;
use workspace::StatusItemView;
use workspace::ToolbarItemEvent;
use workspace::Workspace;
use workspace::item::ItemHandle;
use workspace::status_item_color;

use crate::Deploy;
use crate::ProjectDiagnosticsEditor;

/// The status bar item that displays diagnostic counts.
pub struct DiagnosticIndicator {
    summary: project::DiagnosticSummary,
    workspace: WeakEntity<Workspace>,
    current_diagnostic: Option<Diagnostic>,
    active_editor: Option<WeakEntity<Editor>>,
    /// Whether the project diagnostics view is the active pane item.
    diagnostics_active: bool,
    _observe_active_editor: Option<Subscription>,

    diagnostics_update: Task<()>,
    diagnostic_summary_update: Task<()>,
}

impl Render for DiagnosticIndicator {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let indicator = h_flex().gap_2().min_w_0().overflow_x_hidden();
        if !ProjectSettings::get_global(cx).diagnostics.button {
            return indicator.hidden();
        }

        // The error and warning icons keep their colours; the check icon and
        // the counts take the accent while project diagnostics is active.
        let color = status_item_color(self.diagnostics_active);
        let diagnostic_indicator = match (self.summary.error_count, self.summary.warning_count) {
            (0, 0) => h_flex().child(Icon::new(IconName::Check).size(IconSize::Small).color(color)),
            (error_count, warning_count) => h_flex()
                .gap_1()
                .when(error_count > 0, |this| {
                    this.child(Icon::new(IconName::XCircle).size(IconSize::Small).color(Color::Error))
                        .child(Label::new(error_count.to_string()).size(LabelSize::Small).color(color))
                })
                .when(warning_count > 0, |this| {
                    this.child(Icon::new(IconName::Warning).size(IconSize::Small).color(Color::Warning))
                        .child(Label::new(warning_count.to_string()).size(LabelSize::Small).color(color))
                }),
        };

        let status = if let Some(diagnostic) = &self.current_diagnostic {
            let message =
                diagnostic.message.as_str().split_once('\n').map_or(diagnostic.message.as_str(), |(first, _)| first);
            let diagnostics_already_active = self.any_active_diagnostics(cx);
            let tooltip = if !diagnostics_already_active { "Expand Diagnostics" } else { "Next Diagnostic" };
            Some(
                Button::new("diagnostic_message", SharedString::new(message))
                    .label_size(LabelSize::Small)
                    .truncate(true)
                    .tab_index(0isize)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action(tooltip, &editor::actions::GoToDiagnostic::default(), cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.go_to_next_diagnostic(window, cx))),
            )
        } else {
            None
        };

        indicator
            .child(
                ButtonLike::new("diagnostic-indicator")
                    .child(diagnostic_indicator)
                    .tab_index(0isize)
                    .tooltip(move |_window, cx| Tooltip::for_action("Project Diagnostics", &Deploy, cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(workspace) = this.workspace.upgrade() {
                            workspace.update(cx, |workspace, cx| {
                                ProjectDiagnosticsEditor::deploy(workspace, &Deploy, window, cx)
                            })
                        }
                    })),
            )
            .children(status)
    }
}

impl DiagnosticIndicator {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let project = workspace.project();
        cx.subscribe(project, |this, project, event, cx| match event {
            project::Event::DiskBasedDiagnosticsStarted { .. } => {
                cx.notify();
            }

            project::Event::DiskBasedDiagnosticsFinished { .. } | project::Event::LanguageServerRemoved(_) => {
                this.summary = project.read(cx).diagnostic_summary(false, cx);
                cx.notify();
            }

            project::Event::DiagnosticsUpdated { .. } => {
                this.diagnostic_summary_update = cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_millis(30)).await;
                    this.update(cx, |this, cx| {
                        this.summary = project.read(cx).diagnostic_summary(false, cx);
                        cx.notify();
                    })
                    .log_err();
                });
            }

            _ => {}
        })
        .detach();

        Self {
            summary: project.read(cx).diagnostic_summary(false, cx),
            active_editor: None,
            diagnostics_active: false,
            workspace: workspace.weak_handle(),
            current_diagnostic: None,
            _observe_active_editor: None,
            diagnostics_update: Task::ready(()),
            diagnostic_summary_update: Task::ready(()),
        }
    }

    fn any_active_diagnostics(&self, cx: &Context<Self>) -> bool {
        if let Some(editor) = self.active_editor.as_ref().and_then(|e| e.upgrade()) {
            editor.read(cx).any_active_diagnostics()
        } else {
            false
        }
    }

    fn go_to_next_diagnostic(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor.as_ref().and_then(|e| e.upgrade()) {
            editor.update(cx, |editor, cx| {
                editor.go_to_diagnostic_at_cursor(
                    editor::Direction::Next,
                    GoToDiagnosticSeverityFilter::default(),
                    window,
                    cx,
                );
            })
        }
    }

    fn update(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let (buffer, cursor_position) = editor.update(cx, |editor, cx| {
            let buffer = editor.buffer().read(cx).snapshot(cx);
            let cursor_position = editor.selections.newest::<MultiBufferOffset>(&editor.display_snapshot(cx)).head();
            (buffer, cursor_position)
        });
        let new_diagnostic = buffer
            .diagnostics_in_range::<MultiBufferOffset>(cursor_position..cursor_position)
            .filter(|entry| !entry.range.is_empty())
            .min_by_key(|entry| (entry.diagnostic.severity, entry.range.end - entry.range.start))
            .map(|entry| entry.diagnostic);
        if new_diagnostic != self.current_diagnostic.as_ref() {
            let new_diagnostic = new_diagnostic.cloned();
            self.diagnostics_update = cx.spawn_in(window, async move |diagnostics_indicator, cx| {
                cx.background_executor().timer(Duration::from_millis(50)).await;
                diagnostics_indicator
                    .update(cx, |diagnostics_indicator, cx| {
                        diagnostics_indicator.current_diagnostic = new_diagnostic;
                        cx.notify();
                    })
                    .ok();
            });
        }
    }
}

impl EventEmitter<ToolbarItemEvent> for DiagnosticIndicator {}

impl StatusItemView for DiagnosticIndicator {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.diagnostics_active =
            active_pane_item.is_some_and(|item| item.downcast::<ProjectDiagnosticsEditor>().is_some());
        if let Some(editor) = active_pane_item.and_then(|item| item.downcast::<Editor>()) {
            self.active_editor = Some(editor.downgrade());
            self._observe_active_editor = Some(cx.observe_in(&editor, window, Self::update));
            self.update(editor, window, cx);
        } else {
            self.active_editor = None;
            self.current_diagnostic = None;
            self._observe_active_editor = None;
        }
        cx.notify();
    }
}
