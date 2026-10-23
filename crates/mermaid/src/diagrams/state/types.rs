//! The statements of a state diagram document and the model built from them. Port of Pele's
//! `src/diagrams/state/types.ts`.

/// A state's description: one text, or a title followed by more lines.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Description {
    /// Several texts.
    Many(Vec<String>),
    /// One text.
    One(String),
}

impl Description {
    /// Whether JavaScript reads the value as true: a non-empty string, or any array.
    pub(crate) fn truthy(&self) -> bool {
        match self {
            Self::Many(_) => true,
            Self::One(text) => !text.is_empty(),
        }
    }

    /// The texts, as `[value].flat()` lists them.
    pub(crate) fn texts(&self) -> &[String] {
        match self {
            Self::Many(texts) => texts,
            Self::One(text) => std::slice::from_ref(text),
        }
    }
}

/// The body of a composite state. Dropping it releases nested bodies one at a time, so a deeply
/// nested document cannot overflow the call stack.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Doc(pub(crate) Vec<Stmt>);

impl Drop for Doc {
    fn drop(&mut self) {
        let mut pending = vec![std::mem::take(&mut self.0)];
        while let Some(mut items) = pending.pop() {
            for item in &mut items {
                if let Stmt::State(state) = item
                    && let Some(mut inner) = state.doc.take()
                {
                    pending.push(std::mem::take(&mut inner.0));
                }
            }
        }
    }
}

/// A note beside a state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StateNote {
    /// `left of` or `right of`, as written.
    pub(crate) position: Option<String>,
    /// The note text.
    pub(crate) text: String,
}

/// Pele's `StateStmt`: a state as a statement declares it, or as the model records it. Absent
/// fields are the ones JavaScript leaves `undefined`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StateStmt {
    /// Class names applied to the state.
    pub(crate) classes: Option<Vec<String>>,
    /// The description as written.
    pub(crate) description: Option<Description>,
    /// The descriptions the model collected.
    pub(crate) descriptions: Option<Vec<String>>,
    /// The body of a composite state. A state the model records keeps none: the model records
    /// which root statement's body it is instead.
    pub(crate) doc: Option<Doc>,
    /// The state id.
    pub(crate) id: String,
    /// A note beside the state.
    pub(crate) note: Option<StateNote>,
    /// For `[*]`: whether it starts (rather than ends) its document.
    pub(crate) start: Option<bool>,
    /// Inline styles.
    pub(crate) styles: Option<Vec<String>>,
    /// Text styles.
    pub(crate) text_styles: Option<Vec<String>>,
    /// `default`, `fork`, `join`, `choice`, or `divider`.
    pub(crate) r#type: Option<&'static str>,
}

/// Pele's `RelationStmt`: a transition.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RelationStmt {
    /// The transition label.
    pub(crate) description: Option<String>,
    /// The state the transition leaves.
    pub(crate) state1: StateStmt,
    /// The state the transition reaches.
    pub(crate) state2: StateStmt,
}

/// A statement of a document. Mermaid's grammar also leaves the text of statements that have no
/// effect in a document (`hide empty description`, `scale`, a floating note); those are `Text`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Stmt {
    /// `class ids name`.
    ApplyClass {
        /// The ids, comma-separated.
        id: String,
        /// The class name.
        style_class: String,
    },
    /// `classDef id styles`.
    ClassDef {
        /// The style declarations.
        classes: String,
        /// The class name.
        id: String,
    },
    /// `click id ...`.
    Click {
        /// The state clicked.
        id: StateStmt,
        /// The tooltip, quotes included.
        tooltip: String,
        /// The target, quotes included.
        url: String,
    },
    /// `direction XX`.
    Dir(&'static str),
    /// A transition.
    Relation(Box<RelationStmt>),
    /// A state declaration.
    State(StateStmt),
    /// `style ids styles`.
    Style {
        /// The ids, comma-separated.
        id: String,
        /// The style declarations.
        style_class: String,
    },
    /// Text left by a statement with no effect.
    Text(String),
}

/// Pele's `StateRelation`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StateRelation {
    /// The state the transition leaves.
    pub(crate) id1: String,
    /// The state the transition reaches.
    pub(crate) id2: String,
    /// The transition label.
    pub(crate) relation_title: String,
}

/// Pele's `StateClassDef`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StateClassDef {
    /// The class name.
    pub(crate) id: String,
    /// The style declarations.
    pub(crate) styles: Vec<String>,
    /// The declarations that style text.
    pub(crate) text_styles: Vec<String>,
}

/// Pele's `StateLink`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StateLink {
    /// The tooltip, quotes included.
    pub(crate) tooltip: String,
    /// The target, quotes included.
    pub(crate) url: String,
}
