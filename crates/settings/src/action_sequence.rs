use anyhow::Result;
use gpui::Action;
use gpui::register_action;
use serde_json::Value;

/// Runs actions in order without waiting for asynchronous completion.
/// Construct sequences directly in Rust; dispatch is handled by workspace windows.
pub struct ActionSequence(pub Vec<Box<dyn Action>>);
register_action!(ActionSequence);

impl Action for ActionSequence {
    fn name(&self) -> &'static str {
        Self::name_for_type()
    }

    fn name_for_type() -> &'static str
    where
        Self: Sized,
    {
        "action::Sequence"
    }

    fn partial_eq(&self, action: &dyn Action) -> bool {
        action.as_any().downcast_ref::<Self>().is_some_and(|other| {
            self.0.len() == other.0.len() && self.0.iter().zip(other.0.iter()).all(|(a, b)| a.partial_eq(b.as_ref()))
        })
    }

    fn boxed_clone(&self) -> Box<dyn Action> {
        Box::new(ActionSequence(self.0.iter().map(|action| action.boxed_clone()).collect::<Vec<_>>()))
    }

    fn build(_value: Value) -> Result<Box<dyn Action>> {
        Err(anyhow::anyhow!("{} cannot be built directly", Self::name_for_type()))
    }

    fn deprecated_aliases() -> &'static [&'static str] {
        &[]
    }

    fn deprecation_message() -> Option<&'static str> {
        None
    }

    fn documentation() -> Option<&'static str> {
        Some(
            "Runs a sequence of actions.\n\n\
            NOTE: This does **not** wait for asynchronous actions to complete before running the next action.",
        )
    }
}
