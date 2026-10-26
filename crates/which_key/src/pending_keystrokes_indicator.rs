use std::rc::Rc;
use std::time::Duration;

use gpui::Action as _;
use gpui::Anchor;
use gpui::Animation;
use gpui::AnimationExt;
use gpui::App;
use gpui::Context;
use gpui::HoverListenerMode;
use gpui::InputPreference;
use gpui::KeybindingKeystroke;
use gpui::KeystrokeEvent;
use gpui::Render;
use gpui::ScrollHandle;
use gpui::Subscription;
use gpui::Task;
use gpui::Window;
use gpui::anchored;
use gpui::deferred;
use settings::Settings;
use settings::SettingsStore;
use ui::ButtonLike;
use ui::CircularProgress;
use ui::KeyBinding;
use ui::KeyBindingStyle;
use ui::prelude::*;
use ui::tooltip_container;
use util::ResultExt;
use workspace::StatusBarSettings;
use workspace::StatusItemView;
use workspace::item::ItemHandle;

use crate::ShowPendingBindings;
use crate::bindings_for_pending_input;
use crate::map_pending_keystrokes;
use crate::pending_bindings::PendingBindingRow;
use crate::pending_bindings::PendingBindings;
use crate::pending_bindings::prepare_pending_bindings;

const POPOVER_HIDE_DELAY: Duration = Duration::from_millis(300);

/// A status bar item shown while pending input can complete a multi-stroke key binding.
pub struct PendingKeystrokesIndicator {
    render_state: Option<Rc<IndicatorRenderState>>,
    pending_input_generation: u64,
    popover: PopoverState,
    popover_scroll_handle: ScrollHandle,
    _pending_input_subscription: Subscription,
    _settings_subscription: Subscription,
}

#[derive(Default)]
struct PopoverState {
    indicator_pointer_over: bool,
    pointer_over: bool,
    visible: bool,
    opened_by_keyboard: bool,
    hide_task: Option<Task<()>>,
}

impl PopoverState {
    const fn is_pointer_over(&self) -> bool {
        self.indicator_pointer_over || self.pointer_over
    }
}

struct IndicatorRenderState {
    keystrokes: Rc<[KeybindingKeystroke]>,
    pending_input_generation: u64,
    bindings: Rc<[PendingBindingRow]>,
    timeout: Option<IndicatorTimeout>,
}

struct IndicatorTimeout {
    timeout_duration: Duration,
    remaining_duration: Duration,
    timeout_paused: bool,
}

impl PendingKeystrokesIndicator {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pending_input_subscription = cx.observe_pending_input(window, |this: &mut Self, window, cx| {
            if this.refresh_render_state(window, cx) {
                cx.notify();
            }
            this.update_popover_state(window, cx);
        });

        let mut enabled = Self::enabled(cx);
        let settings_subscription = cx.observe_global_in::<SettingsStore>(window, move |this, window, cx| {
            let new_enabled = Self::enabled(cx);
            if new_enabled == enabled {
                return;
            }

            enabled = new_enabled;
            if this.refresh_render_state(window, cx) {
                cx.notify();
            }
            this.update_popover_state(window, cx);
        });

        Self {
            render_state: None,
            pending_input_generation: 0,
            popover: PopoverState::default(),
            popover_scroll_handle: ScrollHandle::new(),
            _pending_input_subscription: pending_input_subscription,
            _settings_subscription: settings_subscription,
        }
    }

    fn enabled(cx: &App) -> bool {
        let status_bar_settings = StatusBarSettings::get_global(cx);
        status_bar_settings.show && status_bar_settings.pending_keystrokes_indicator
    }

    fn refresh_render_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !Self::enabled(cx) {
            return self.clear_render_state(window, cx);
        }

        let Some(pending_input) = window.pending_input() else {
            return self.clear_render_state(window, cx);
        };
        let keystrokes = pending_input.keystrokes();

        let bindings = prepare_pending_bindings(bindings_for_pending_input(window, keystrokes), cx);

        let keystrokes = map_pending_keystrokes(keystrokes, cx.keyboard_mapper().as_ref());
        let pending_keys_changed =
            self.render_state.as_ref().is_none_or(|previous| previous.keystrokes.as_ref() != keystrokes.as_slice());
        if pending_keys_changed {
            self.popover_scroll_handle.set_offset(gpui::Point::default());
        }
        // Pausing or resuming the timeout also notifies observers, so close a hover-opened
        // popover early only when the pending keys change. A keyboard-opened popover stays open
        // while any input is pending, even if an unmatched key resolves one chord and starts
        // another.
        if self.popover.visible
            && !self.popover.opened_by_keyboard
            && !self.popover.is_pointer_over()
            && pending_keys_changed
        {
            self.popover = PopoverState::default();
        }

        self.pending_input_generation = self.pending_input_generation.wrapping_add(1);
        self.render_state = Some(Rc::new(IndicatorRenderState {
            keystrokes: keystrokes.into(),
            pending_input_generation: self.pending_input_generation,
            bindings: bindings.into(),
            timeout: pending_input.timeout().map(|timeout| IndicatorTimeout {
                timeout_duration: timeout.duration(),
                remaining_duration: timeout.remaining(cx),
                timeout_paused: timeout.is_paused(),
            }),
        }));
        true
    }

    fn clear_render_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.popover = PopoverState::default();
        self.popover_scroll_handle.set_offset(gpui::Point::default());
        window.set_pending_input_timeout_paused(&cx.entity(), false, cx);
        self.render_state.take().is_some()
    }

    const fn render_state(&self) -> Option<&Rc<IndicatorRenderState>> {
        self.render_state.as_ref()
    }

    fn set_indicator_pointer_over(&mut self, pointer_over: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.popover.indicator_pointer_over = pointer_over;
        self.update_popover_state(window, cx);
    }

    fn set_popover_pointer_over(&mut self, pointer_over: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.popover.pointer_over = pointer_over;
        self.update_popover_state(window, cx);
    }

    fn update_popover_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.render_state.is_none() {
            return;
        }

        if self.popover.opened_by_keyboard || self.popover.is_pointer_over() {
            self.popover.hide_task.take();
            let was_visible = self.popover.visible;
            self.popover.visible = true;
            window.set_pending_input_timeout_paused(&cx.entity(), true, cx);
            if was_visible != self.popover.visible {
                cx.notify();
            }
        } else if self.popover.visible && self.popover.hide_task.is_none() {
            window.set_pending_input_timeout_paused(&cx.entity(), true, cx);
            self.popover.hide_task = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(POPOVER_HIDE_DELAY).await;
                this.update_in(cx, |this, window, cx| {
                    this.popover.hide_task.take();
                    if this.popover.is_pointer_over() {
                        return;
                    }

                    this.popover.visible = false;
                    window.set_pending_input_timeout_paused(&cx.entity(), false, cx);
                    cx.notify();
                })
                .log_err();
            }));
        } else if !self.popover.visible {
            window.set_pending_input_timeout_paused(&cx.entity(), false, cx);
        }
    }

    fn handle_intercepted_keystroke(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.input_preference == InputPreference::CharacterInput
            || !window.has_pending_keystrokes()
            || !Self::enabled(cx)
        {
            return;
        }

        // Only the highest-precedence binding counts, so user overrides can disable the shortcut.
        let (bindings, _) =
            cx.key_bindings().borrow().bindings_for_input(std::slice::from_ref(&event.keystroke), &event.context_stack);
        if !bindings.first().is_some_and(|binding| binding.action().partial_eq(&ShowPendingBindings)) {
            return;
        }

        cx.stop_propagation();
        self.popover.opened_by_keyboard = true;
        self.update_popover_state(window, cx);
        cx.notify();
    }
}

impl Render for PendingKeystrokesIndicator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(render_state) = self.render_state().cloned() else {
            return div().hidden().into_any_element();
        };

        let window_handle = window.window_handle();
        let listener = cx.listener(move |this, event: &KeystrokeEvent, window, cx| {
            if window.window_handle() == window_handle {
                this.handle_intercepted_keystroke(event, window, cx);
            }
        });
        // Inactive workspaces in the same window keep their indicators alive. Storing the
        // subscription in element state ensures only the rendered indicator intercepts input.
        window.use_keyed_state("pending-keystrokes-interceptor", cx, |_, cx| cx.intercept_keystrokes(listener));

        let button = ButtonLike::new("pending-keystrokes-indicator")
            .on_click(|_, window, cx| {
                window.dispatch_action(ed_actions::dev::OpenKeyContextView.boxed_clone(), cx);
            })
            .when_some(render_state.timeout.as_ref(), |button, timeout| {
                let remaining_fraction = if timeout.timeout_duration.is_zero() {
                    0.0
                } else {
                    (timeout.remaining_duration.as_secs_f32() / timeout.timeout_duration.as_secs_f32()).clamp(0.0, 1.0)
                };
                button.child(if cx.reduce_motion() {
                    Icon::new(IconName::CountdownTimer).size(IconSize::XSmall).color(Color::Muted).into_any_element()
                } else {
                    let progress = CircularProgress::new(
                        remaining_fraction,
                        1.0,
                        rems_from_px(13_f32).to_pixels(window.rem_size()),
                        cx,
                    )
                    .stroke_width(rems_from_px(2_f32).to_pixels(window.rem_size()))
                    .progress_color(cx.theme().colors().text_muted);
                    if timeout.timeout_paused || timeout.remaining_duration.is_zero() {
                        progress.into_any_element()
                    } else {
                        progress
                            .with_animation(
                                ("pending-keystrokes-countdown", render_state.pending_input_generation),
                                Animation::new(timeout.remaining_duration).with_max_fps(30.0),
                                move |progress, delta| progress.value(remaining_fraction * (1.0 - delta)),
                            )
                            .into_any_element()
                    }
                })
            })
            .child(
                KeyBinding::from_keystrokes(render_state.keystrokes.clone())
                    .size(rems_from_px(12_f32))
                    .style(KeyBindingStyle::Label),
            );

        let popover = self.popover.visible.then(|| {
            let popover_render_state = render_state.clone();
            let viewport_size = window.viewport_size();
            let max_panel_width = px((f32::from(viewport_size.width) * 0.5).min(480.0));
            let max_content_height = px(f32::from(viewport_size.height) * 0.4);
            let anchored_popover = deferred(
                anchored().anchor(Anchor::BottomRight).snap_to_window_with_margin(px(8.)).child(
                    div()
                        .id("pending-keystrokes-popover")
                        .pb_2()
                        .occlude()
                        .on_hover(cx.listener(|this, pointer_over: &bool, window, cx| {
                            this.set_popover_pointer_over(*pointer_over, window, cx);
                        }))
                        .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
                        .child(tooltip_container(cx, |el, _| {
                            el.p_0().max_w(max_panel_width).overflow_hidden().child(PendingBindings::new(
                                "pending-keystrokes-popover-content",
                                popover_render_state.keystrokes.clone(),
                                popover_render_state.bindings.clone(),
                                self.popover_scroll_handle.clone(),
                                max_panel_width,
                                max_content_height,
                            ))
                        })),
                ),
            )
            .with_priority(1);

            div().absolute().top_0().right_0().w_0().h_0().child(anchored_popover)
        });

        div()
            .id("pending-keystrokes-indicator-wrapper")
            .relative()
            .child(button)
            .when_some(popover, |this, popover| this.child(popover))
            .on_hover(cx.listener(|this, pointer_over: &bool, window, cx| {
                this.set_indicator_pointer_over(*pointer_over, window, cx);
            }))
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .into_any_element()
    }
}

impl StatusItemView for PendingKeystrokesIndicator {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}
