use settings_content::{RootUserSettings, UserSettingsContent};

#[test]
fn obsolete_feature_and_channel_settings_do_not_override_editor_settings() {
    let settings = UserSettingsContent::parse_json_with_comments(
        r#"{
            "cursor_blink": true,
            "feature_flags": {"notebooks": "off", "panic": "on", "diff-review": "on"},
            "dev": {"cursor_blink": false},
            "stable": {"cursor_blink": false},
            "macos": {"cursor_blink": false}
        }"#,
    )
    .unwrap();

    assert_eq!(settings.content.editor.cursor_blink, Some(true));
    assert_eq!(
        settings
            .platform_overrides
            .get_by_key("macos")
            .unwrap()
            .editor
            .cursor_blink,
        Some(false)
    );

    let value = serde_json::to_value(settings).unwrap();
    for key in ["feature_flags", "dev", "nightly", "preview", "stable"] {
        assert!(
            value.get(key).is_none(),
            "obsolete setting {key} was retained"
        );
    }
}

#[test]
fn settings_schema_does_not_expose_feature_or_channel_overrides() {
    let schema = schemars::schema_for!(UserSettingsContent);
    let value = serde_json::to_value(schema).unwrap();
    let properties = value["properties"].as_object().unwrap();
    assert!(properties.contains_key("cursor_blink"));
    assert!(properties.contains_key("macos"));
    for key in ["feature_flags", "dev", "nightly", "preview", "stable"] {
        assert!(
            !properties.contains_key(key),
            "obsolete setting {key} is in the schema"
        );
    }
}
