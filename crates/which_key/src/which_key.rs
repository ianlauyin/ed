//! Pending multi-stroke key binding hints.

mod pending_bindings;
mod pending_keystrokes_indicator;

use gpui::Action;
use gpui::KeybindingKeystroke;
use gpui::Keystroke;
use gpui::PlatformKeyboardMapper;
use gpui::SharedString;
use gpui::Window;
pub use pending_keystrokes_indicator::PendingKeystrokesIndicator;

gpui::actions!(
    which_key,
    [
        /// Shows the bindings that can complete the pending key sequence. Must be bound to a
        /// single keystroke.
        ShowPendingBindings
    ]
);

pub(crate) struct PendingBinding {
    pub(crate) remaining_keystrokes: Vec<KeybindingKeystroke>,
    pub(crate) action_name: SharedString,
}

pub(crate) fn map_pending_keystrokes(
    keystrokes: &[Keystroke],
    keyboard_mapper: &dyn PlatformKeyboardMapper,
) -> Vec<KeybindingKeystroke> {
    keystrokes
        .iter()
        .cloned()
        .map(|keystroke| KeybindingKeystroke::new_with_mapper(keystroke, false, keyboard_mapper))
        .collect()
}

pub(crate) fn bindings_for_pending_input(window: &Window, pending_keystrokes: &[Keystroke]) -> Vec<PendingBinding> {
    // Uses GPUI's candidate list as is, on purpose, instead of repeating dispatch rules.
    // The list can include a longer chord that can't be typed because a shorter binding for its
    // prefix comes later in the keymap. GPUI's shortcut display shows those chords too.
    window
        .possible_bindings_for_input(pending_keystrokes)
        .into_iter()
        .filter_map(|binding| {
            let remaining_keystrokes = binding.keystrokes().get(pending_keystrokes.len()..)?;
            if remaining_keystrokes.is_empty() {
                return None;
            }
            let remaining_keystrokes = remaining_keystrokes.to_vec();
            let action_name = binding_label(binding.action());
            Some(PendingBinding { remaining_keystrokes, action_name })
        })
        .collect()
}

fn binding_label(action: &dyn Action) -> SharedString {
    command_palette::humanize_action_name(action.name()).into()
}
