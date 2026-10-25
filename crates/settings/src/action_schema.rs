//! Schema helpers for action inputs (independent of keymap configuration).
use gpui::generate_list_of_all_registered_actions;
use util::schemars::AllowTrailingCommas;

pub fn action_schema_generator() -> schemars::SchemaGenerator {
    schemars::generate::SchemaSettings::draft2020_12().with_transform(AllowTrailingCommas).into_generator()
}

pub fn get_action_schema_by_name(
    action_name: &str,
    generator: &mut schemars::SchemaGenerator,
) -> Option<schemars::Schema> {
    for action_data in generate_list_of_all_registered_actions() {
        if action_data.name == action_name {
            return (action_data.json_schema)(generator);
        }
        for &alias in action_data.deprecated_aliases {
            if alias == action_name {
                return (action_data.json_schema)(generator);
            }
        }
    }
    None
}
