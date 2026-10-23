//! The records of a class diagram. Port of Pele's `src/diagrams/class/types.ts`.

use super::members::ClassMember;

/// An end of a relation: Mermaid's relation type number, or `none`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RelationEnd {
    /// No marker.
    None,
    /// Mermaid's relation type: 0 aggregation, 1 extension, 2 composition, 3 dependency,
    /// 4 lollipop.
    Type(u8),
}

/// Mermaid's lollipop relation type.
pub(crate) const LOLLIPOP: RelationEnd = RelationEnd::Type(4);

/// A class.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClassNode {
    /// Annotations such as `interface`.
    pub(crate) annotations: Vec<String>,
    /// Class names, starting with `default`, separated by spaces.
    pub(crate) css_classes: String,
    /// Set when a callback or `click ... call` names the class.
    pub(crate) have_callback: bool,
    /// The class name, without its generic type.
    pub(crate) id: String,
    /// The shown name.
    pub(crate) label: String,
    /// The link address.
    pub(crate) link: Option<String>,
    /// The link target, once a link is set.
    pub(crate) link_target: Option<String>,
    /// Attributes.
    pub(crate) members: Vec<ClassMember>,
    /// Methods.
    pub(crate) methods: Vec<ClassMember>,
    /// The namespace the class is declared in.
    pub(crate) parent: Option<String>,
    /// Style declarations from `style` statements and class definitions.
    pub(crate) styles: Vec<String>,
    /// The tooltip.
    pub(crate) tooltip: Option<String>,
    /// The generic type, such as `T` for `List~T~`.
    pub(crate) r#type: String,
}

/// A relation between two classes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClassRelation {
    /// The first class, or the interface standing for a lollipop end.
    pub(crate) id1: String,
    /// The second class, or the interface standing for a lollipop end.
    pub(crate) id2: String,
    /// The line style: 0 solid, 1 dotted.
    pub(crate) line_type: u8,
    /// The text at the first end, or `none`.
    pub(crate) relation_title1: String,
    /// The text at the second end, or `none`.
    pub(crate) relation_title2: String,
    /// The label.
    pub(crate) title: Option<String>,
    /// The marker at the first end.
    pub(crate) type1: RelationEnd,
    /// The marker at the second end.
    pub(crate) type2: RelationEnd,
}

/// A note, attached to a class or free.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClassNote {
    /// The class the note is for, as written.
    pub(crate) class: Option<String>,
    /// `note` followed by the index.
    pub(crate) id: String,
    /// Position among the notes.
    pub(crate) index: usize,
    /// The namespace the note is declared in.
    pub(crate) parent: Option<String>,
    /// The text.
    pub(crate) text: String,
}

/// The interface a lollipop end stands for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClassInterface {
    /// The class the lollipop is on.
    pub(crate) class_id: String,
    /// `interface` followed by the index.
    pub(crate) id: String,
    /// The shown name.
    pub(crate) label: String,
}

/// A namespace.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NamespaceNode {
    /// Child namespaces, by id, in insertion order.
    pub(crate) children: Vec<String>,
    /// Classes declared in it, by id, in insertion order.
    pub(crate) classes: Vec<String>,
    /// False for the ancestors created on the way to a dotted name such as `A.B.C`.
    pub(crate) explicit: bool,
    /// The full dotted name.
    pub(crate) id: String,
    /// The shown name.
    pub(crate) label: String,
    /// Notes declared in it, by id, in insertion order.
    pub(crate) notes: Vec<String>,
    /// The enclosing namespace.
    pub(crate) parent: Option<String>,
}

/// A class definition from `classDef`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StyleClass {
    /// The class name.
    pub(crate) id: String,
    /// Its declarations.
    pub(crate) styles: Vec<String>,
    /// Its declarations that mention a color, with `fill` read as `bgFill`.
    pub(crate) text_styles: Vec<String>,
}
