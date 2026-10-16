use collections::HashMap;
use gpui::{
    Action as _, App, KeyBinding, KeyBindingContextPredicate, KeyBindingMetaIndex, NoAction,
};
use serde_json::Value;
use std::rc::Rc;

/// A binding constructed in Rust. Contexts and keyboard-position mapping retain
/// GPUI's normal semantics; later bindings take precedence at the same depth.
#[derive(Clone, Debug)]
pub struct KeyBindingSpec {
    pub keystrokes: &'static str,
    pub action: Option<&'static str>,
    pub input: Option<Value>,
    pub context: Option<&'static str>,
    pub use_key_equivalents: bool,
    /// Explicit overrides survive more specific built-in disabled bindings.
    pub is_override: bool,
}

impl KeyBindingSpec {
    pub fn new(keystrokes: &'static str, action: &'static str) -> Self {
        Self {
            keystrokes,
            action: Some(action),
            input: None,
            context: None,
            use_key_equivalents: false,
            is_override: false,
        }
    }

    pub fn disabled(keystrokes: &'static str) -> Self {
        Self {
            action: None,
            ..Self::new(keystrokes, "")
        }
    }

    pub fn with_override(mut self) -> Self {
        self.is_override = true;
        self
    }

    pub fn with_input(mut self, input: Value) -> Self {
        self.input = Some(input);
        self
    }
}

/// Build the application keymap using the current keyboard layout.
pub fn built_in_key_bindings(cx: &App) -> Vec<KeyBinding> {
    let mut contexts = HashMap::<&'static str, Rc<KeyBindingContextPredicate>>::default();
    crate::built_in_keymap::keymap()
        .into_iter()
        .map(|binding| {
            let action = match binding.action {
                Some(name) => cx
                    .build_action(name, binding.input.clone())
                    .unwrap_or_else(|error| panic!("invalid built-in action {name}: {error}")),
                None => NoAction.boxed_clone(),
            };
            let context = binding.context.map(|context| {
                contexts
                    .entry(context)
                    .or_insert_with(|| {
                        KeyBindingContextPredicate::parse(context)
                            .unwrap_or_else(|error| {
                                panic!("invalid built-in context {context}: {error}")
                            })
                            .into()
                    })
                    .clone()
            });
            KeyBinding::load(
                binding.keystrokes,
                action,
                context,
                binding.use_key_equivalents,
                binding.input.map(|input| input.to_string().into()),
                cx.keyboard_mapper().as_ref(),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "invalid built-in keystrokes {}: {error}",
                    binding.keystrokes
                )
            })
            .with_meta(KeyBindingMetaIndex(if binding.is_override { 0 } else { 2 }))
        })
        .collect()
}
