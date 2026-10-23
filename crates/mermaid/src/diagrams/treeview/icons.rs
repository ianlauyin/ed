//! Which icon each tree view entry shows. Port of Pele's `src/diagrams/treeview/icons.ts`.

use super::model::{NodeType, TreeNode};
use crate::yaml::Value;

/// Mermaid's name for its built-in pack. Only `file` and `folder` are built in; every other icon
/// name is offered to the host's icon resolver.
pub(crate) const BUILTIN_PACK: &str = "mermaid-treeview";

/// The built-in outline named `name`, on a 24 by 24 grid.
pub(crate) fn glyph(name: &str) -> Option<&'static str> {
    match name {
        "file" => Some(
            "M6.5 3h7L19 8.5v11a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 5 19.5v-15A1.5 1.5 0 0 1 6.5 3zM13.5 3v5.5H19",
        ),
        "folder" => Some(
            "M3 6.5A1.5 1.5 0 0 1 4.5 5h4.6l2.4 2.5h8A1.5 1.5 0 0 1 21 9v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z",
        ),
        _ => None,
    }
}

/// The icon options of the `treeView` config section: Pele's `IconConfig`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IconConfig<'a> {
    /// The pack of an icon name written without one.
    pub(crate) default_icon_pack: &'a str,
    /// Icons by file extension.
    pub(crate) extension_icons: Option<&'a [(String, Value)]>,
    /// Icons by file name.
    pub(crate) filename_icons: Option<&'a [(String, Value)]>,
    /// Whether entries without an `icon()` show one.
    pub(crate) show_icons: bool,
}

/// The entries of `value` when it is a mapping.
fn map(value: Option<&Value>) -> Option<&[(String, Value)]> {
    match value {
        Some(Value::Object(entries)) => Some(entries),
        _ => None,
    }
}

/// The string at `key` of `map`, as an own property.
fn own<'a>(map: Option<&'a [(String, Value)]>, key: &str) -> Option<&'a str> {
    match map?.iter().find(|(own, _)| own == key) {
        Some((_, Value::String(value))) => Some(value),
        _ => None,
    }
}

/// `text.toLowerCase()`.
fn lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// A file's icon from the configured maps. A file name wins over an extension, and extensions
/// match in lower case with or without the leading dot.
pub(crate) fn detect_icon<'a>(name: &str, config: &IconConfig<'a>) -> Option<&'a str> {
    if let Some(by_name) = own(config.filename_icons, name).filter(|icon| !icon.is_empty()) {
        return Some(by_name);
    }
    let dot = name.rfind('.').filter(|&dot| dot > 0)?;
    let ext = lowercase(name.get(dot..).unwrap_or(""));
    own(config.extension_icons, &ext).or_else(|| own(config.extension_icons, ext.get(1..)?))
}

/// The icon name with its pack.
fn qualify_icon(icon: &str, default_icon_pack: &str) -> String {
    if icon.contains(':') {
        return icon.to_owned();
    }
    let pack = if glyph(icon).is_some() || default_icon_pack.is_empty() {
        BUILTIN_PACK
    } else {
        default_icon_pack
    };
    let mut out = String::from(pack);
    out.push(':');
    out.push_str(icon);
    out
}

/// The qualified icon name for a node, or `None` when it shows none. An `icon()` annotation
/// always wins; otherwise icons appear only with `showIcons`.
pub(crate) fn get_node_icon(node: &TreeNode, config: &IconConfig<'_>) -> Option<String> {
    match node.icon.as_deref() {
        Some("none") => return None,
        Some(icon) if !icon.is_empty() => {
            return Some(qualify_icon(icon, config.default_icon_pack));
        }
        _ => {}
    }
    if !config.show_icons {
        return None;
    }
    if node.node_type == NodeType::File {
        match detect_icon(&node.name, config) {
            Some("none") => return None,
            Some(detected) if !detected.is_empty() => {
                return Some(qualify_icon(detected, config.default_icon_pack));
            }
            _ => {}
        }
    }
    let mut out = String::from(BUILTIN_PACK);
    out.push_str(if node.node_type == NodeType::Directory {
        ":folder"
    } else {
        ":file"
    });
    Some(out)
}

/// The icon options of `config`, as Pele's `iconConfig` reads them.
pub(crate) fn icon_config(config: &Value) -> IconConfig<'_> {
    let section = match config {
        Value::Object(entries) => map(entries
            .iter()
            .find(|(key, _)| key == "treeView")
            .map(|(_, value)| value)),
        _ => None,
    };
    let get = |key: &str| {
        section.and_then(|entries| {
            entries
                .iter()
                .find(|(own, _)| own == key)
                .map(|(_, value)| value)
        })
    };
    IconConfig {
        default_icon_pack: match get("defaultIconPack") {
            Some(Value::String(pack)) => pack,
            _ => "",
        },
        extension_icons: map(get("extensionIcons")),
        filename_icons: map(get("filenameIcons")),
        show_icons: get("showIcons").is_some_and(Value::truthy),
    }
}
