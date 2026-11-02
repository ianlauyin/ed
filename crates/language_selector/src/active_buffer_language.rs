use editor::Editor;
use gpui::Context;
use gpui::Entity;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Render;
use gpui::Styled;
use gpui::Subscription;
use gpui::WeakEntity;
use gpui::Window;
use gpui::div;
use language::LanguageName;
use language::PLAIN_TEXT;
use settings::Settings as _;
use ui::Button;
use ui::ButtonCommon;
use ui::Clickable;
use ui::FluentBuilder;
use ui::LabelSize;
use ui::Tooltip;
use workspace::StatusBarSettings;
use workspace::StatusItemView;
use workspace::Workspace;
use workspace::item::ItemHandle;

use crate::LanguageSelector;
use crate::Toggle;

pub struct ActiveBufferLanguage {
    active_language: Option<Option<LanguageName>>,
    workspace: WeakEntity<Workspace>,
    _observe_active_editor: Option<Subscription>,
}

impl ActiveBufferLanguage {
    pub fn new(workspace: &Workspace) -> Self {
        Self { active_language: None, workspace: workspace.weak_handle(), _observe_active_editor: None }
    }

    fn update_language(&mut self, editor: Entity<Editor>, _: &mut Window, cx: &mut Context<Self>) {
        self.active_language = Some(None);

        let editor = editor.read(cx);
        if let Some(buffer) = editor.active_buffer(cx) {
            let buffer = buffer.read(cx);
            let language =
                buffer.language().filter(|language| **language != *PLAIN_TEXT).map(|language| language.name());
            // Files with no registered language (e.g. TOML) still show
            // the name of the syntax they are highlighted with.
            self.active_language = Some(language.or_else(|| {
                buffer.fallback_syntax_name().map(LanguageName::new).or_else(|| Some(buffer.language()?.name()))
            }));
        }

        cx.notify();
    }
}

impl Render for ActiveBufferLanguage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !StatusBarSettings::get_global(cx).active_language_button {
            return div().hidden();
        }

        div().when_some(self.active_language.as_ref(), |el, active_language| {
            let active_language_text = if let Some(active_language_text) = active_language {
                active_language_text.to_string()
            } else {
                "Unknown".to_string()
            };

            el.child(
                Button::new("change-language", active_language_text)
                    .label_size(LabelSize::Small)
                    .tab_index(0isize)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(workspace) = this.workspace.upgrade() {
                            workspace.update(cx, |workspace, cx| LanguageSelector::toggle(workspace, window, cx));
                        }
                    }))
                    .tooltip(|_window, cx| Tooltip::for_action("Select Language", &Toggle, cx)),
            )
        })
    }
}

impl StatusItemView for ActiveBufferLanguage {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = active_pane_item.and_then(|item| item.act_as::<Editor>(cx)) {
            self._observe_active_editor = Some(cx.observe_in(&editor, window, Self::update_language));
            self.update_language(editor, window, cx);
        } else {
            self.active_language = None;
            self._observe_active_editor = None;
        }

        cx.notify();
    }
}
