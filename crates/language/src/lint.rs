/// Project state file whose `lint` section switches linters on and off.
pub const WORKSPACE_STATE_FILE: &str = ".ed/workspace.json";

/// A `lint.<key>` switch and whether it is on when unset.
pub type LintSwitch = (&'static str, bool);

pub const CLIPPY: LintSwitch = ("clippy", true);
pub const STYLELINT: LintSwitch = ("stylelint", true);
pub const BIOME: LintSwitch = ("biome", false);

/// Every switch, written to the `lint` section with its default when missing.
pub const LINT_SWITCHES: &[LintSwitch] = &[CLIPPY, STYLELINT, BIOME];

/// Whether the `lint.<key>` switch in the workspace state `text` is on. A missing file, key or
/// anything but `"on"` / `"off"` means `default`.
pub fn lint_enabled(text: Option<&str>, key: &str, default: bool) -> bool {
    let state: serde_json::Value = text.and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default();
    match state.pointer(&format!("/lint/{key}")).and_then(|value| value.as_str()) {
        Some("on") => true,
        Some("off") => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::LINT_SWITCHES;
    use super::lint_enabled;

    #[test]
    fn switches_have_unique_keys() {
        let mut keys: Vec<_> = LINT_SWITCHES.iter().map(|(key, _)| *key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), LINT_SWITCHES.len());
    }

    #[test]
    fn switch_overrides_the_default() {
        let text = r#"{ "lint": { "biome": "on", "clippy": "off", "tombi": "maybe" } }"#;
        assert!(lint_enabled(Some(text), "biome", false));
        assert!(!lint_enabled(Some(text), "clippy", true));
        assert!(lint_enabled(Some(text), "tombi", true));
        assert!(!lint_enabled(Some(text), "tombi", false));
    }

    #[test]
    fn missing_or_broken_state_means_default() {
        assert!(lint_enabled(None, "biome", true));
        assert!(!lint_enabled(Some("not json"), "biome", false));
        assert!(lint_enabled(Some(r#"{ "docks": {} }"#), "stylelint", true));
    }
}
