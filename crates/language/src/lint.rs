/// Project state file whose `lint` section switches linters on and off.
pub const WORKSPACE_STATE_FILE: &str = ".ed/workspace.json";

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
    use super::lint_enabled;

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
