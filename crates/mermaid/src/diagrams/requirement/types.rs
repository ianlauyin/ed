//! The records of a requirement diagram. Port of Pele's `src/diagrams/requirement/types.ts`.

/// A requirement.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Requirement {
    /// Class names, starting with `default`.
    pub(crate) classes: Vec<String>,
    /// Style declarations from `style` statements and classes.
    pub(crate) css_styles: Vec<String>,
    /// The requirement's name.
    pub(crate) name: String,
    /// The `id` field.
    #[expect(clippy::struct_field_names, reason = "mirrors Pele's `requirementId`")]
    pub(crate) requirement_id: String,
    /// The `risk` field.
    pub(crate) risk: String,
    /// The `text` field.
    pub(crate) text: String,
    /// Its kind, such as `Functional Requirement`.
    pub(crate) r#type: String,
    /// The `verifymethod` field.
    pub(crate) verify_method: String,
}

/// An element.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RequirementElement {
    /// Class names, starting with `default`.
    pub(crate) classes: Vec<String>,
    /// Style declarations from `style` statements and classes.
    pub(crate) css_styles: Vec<String>,
    /// The `docref` field.
    pub(crate) doc_ref: String,
    /// The element's name.
    pub(crate) name: String,
    /// The `type` field.
    pub(crate) r#type: String,
}

/// A relationship between two names.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Relation {
    /// The destination name.
    pub(crate) dst: String,
    /// The source name.
    pub(crate) src: String,
    /// The relationship, such as `contains`.
    pub(crate) r#type: String,
}

/// A class from `classDef`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RequirementClass {
    /// The class name.
    pub(crate) id: String,
    /// Its declarations.
    pub(crate) styles: Vec<String>,
    /// Its declarations that mention a color, with `fill` read as `bgFill`.
    pub(crate) text_styles: Vec<String>,
}
