//! C4 token kinds. Port of Pele's `src/diagrams/c4/tokens.ts`.

/// A token kind. The order mirrors Pele's `T` enum, and the parser compares kinds by it, so the
/// derived order is observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum T {
    /// After the end of input.
    End,
    /// Unrecognized text.
    Error,
    /// `direction TB`.
    DirectionTb,
    /// `direction BT`.
    DirectionBt,
    /// `direction RL`.
    DirectionRl,
    /// `direction LR`.
    DirectionLr,
    /// `title` and its text.
    Title,
    /// `accDescription` and its text.
    AccDescription,
    /// `accTitle:`.
    AccTitle,
    /// The text of an accessible title.
    AccTitleValue,
    /// `accDescr:`.
    AccDescr,
    /// The text of an accessible description.
    AccDescrValue,
    /// The text of a braced accessible description.
    AccDescrMultilineValue,
    /// Line breaks.
    Newline,
    /// `C4Context`.
    C4Context,
    /// `C4Container`.
    C4Container,
    /// `C4Component`.
    C4Component,
    /// `C4Dynamic`.
    C4Dynamic,
    /// `C4Deployment`.
    C4Deployment,
    /// `Person`.
    Person,
    /// `Person_Ext`.
    PersonExt,
    /// `System`.
    System,
    /// `SystemDb`.
    SystemDb,
    /// `SystemQueue`.
    SystemQueue,
    /// `System_Ext`.
    SystemExt,
    /// `SystemDb_Ext`.
    SystemExtDb,
    /// `SystemQueue_Ext`.
    SystemExtQueue,
    /// `Container`.
    Container,
    /// `ContainerDb`.
    ContainerDb,
    /// `ContainerQueue`.
    ContainerQueue,
    /// `Container_Ext`.
    ContainerExt,
    /// `ContainerDb_Ext`.
    ContainerExtDb,
    /// `ContainerQueue_Ext`.
    ContainerExtQueue,
    /// `Component`.
    Component,
    /// `ComponentDb`.
    ComponentDb,
    /// `ComponentQueue`.
    ComponentQueue,
    /// `Component_Ext`.
    ComponentExt,
    /// `ComponentDb_Ext`.
    ComponentExtDb,
    /// `ComponentQueue_Ext`.
    ComponentExtQueue,
    /// `Enterprise_Boundary`.
    EnterpriseBoundary,
    /// `System_Boundary`.
    SystemBoundary,
    /// `Boundary`.
    Boundary,
    /// `Container_Boundary`.
    ContainerBoundary,
    /// `Deployment_Node` or `Node`.
    Node,
    /// `Node_L`.
    NodeL,
    /// `Node_R`.
    NodeR,
    /// `Rel`.
    Rel,
    /// `BiRel`.
    Birel,
    /// `Rel_Up` or `Rel_U`.
    RelU,
    /// `Rel_Down` or `Rel_D`.
    RelD,
    /// `Rel_Left` or `Rel_L`.
    RelL,
    /// `Rel_Right` or `Rel_R`.
    RelR,
    /// `Rel_Back`.
    RelB,
    /// `RelIndex`.
    RelIndex,
    /// `UpdateElementStyle`.
    UpdateElStyle,
    /// `UpdateRelStyle`.
    UpdateRelStyle,
    /// `UpdateLayoutConfig`.
    UpdateLayoutConfig,
    /// The end of input inside a statement's name.
    EofInStruct,
    /// An empty argument.
    AttributeEmpty,
    /// An argument.
    Str,
    /// The name of a named argument.
    StrKey,
    /// The value of a named argument.
    StrValue,
    /// `{`.
    Lbrace,
    /// `}`.
    Rbrace,
    /// The end of input.
    Eof,
}

impl T {
    /// The name Mermaid's parser reports, from Pele's `TOKEN_NAMES`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::End => "$end",
            Self::Error => "INVALID",
            Self::DirectionTb => "direction_tb",
            Self::DirectionBt => "direction_bt",
            Self::DirectionRl => "direction_rl",
            Self::DirectionLr => "direction_lr",
            Self::Title => "title",
            Self::AccDescription => "accDescription",
            Self::AccTitle => "acc_title",
            Self::AccTitleValue => "acc_title_value",
            Self::AccDescr => "acc_descr",
            Self::AccDescrValue => "acc_descr_value",
            Self::AccDescrMultilineValue => "acc_descr_multiline_value",
            Self::Newline => "NEWLINE",
            Self::C4Context => "C4_CONTEXT",
            Self::C4Container => "C4_CONTAINER",
            Self::C4Component => "C4_COMPONENT",
            Self::C4Dynamic => "C4_DYNAMIC",
            Self::C4Deployment => "C4_DEPLOYMENT",
            Self::Person => "PERSON",
            Self::PersonExt => "PERSON_EXT",
            Self::System => "SYSTEM",
            Self::SystemDb => "SYSTEM_DB",
            Self::SystemQueue => "SYSTEM_QUEUE",
            Self::SystemExt => "SYSTEM_EXT",
            Self::SystemExtDb => "SYSTEM_EXT_DB",
            Self::SystemExtQueue => "SYSTEM_EXT_QUEUE",
            Self::Container => "CONTAINER",
            Self::ContainerDb => "CONTAINER_DB",
            Self::ContainerQueue => "CONTAINER_QUEUE",
            Self::ContainerExt => "CONTAINER_EXT",
            Self::ContainerExtDb => "CONTAINER_EXT_DB",
            Self::ContainerExtQueue => "CONTAINER_EXT_QUEUE",
            Self::Component => "COMPONENT",
            Self::ComponentDb => "COMPONENT_DB",
            Self::ComponentQueue => "COMPONENT_QUEUE",
            Self::ComponentExt => "COMPONENT_EXT",
            Self::ComponentExtDb => "COMPONENT_EXT_DB",
            Self::ComponentExtQueue => "COMPONENT_EXT_QUEUE",
            Self::EnterpriseBoundary => "ENTERPRISE_BOUNDARY",
            Self::SystemBoundary => "SYSTEM_BOUNDARY",
            Self::Boundary => "BOUNDARY",
            Self::ContainerBoundary => "CONTAINER_BOUNDARY",
            Self::Node => "NODE",
            Self::NodeL => "NODE_L",
            Self::NodeR => "NODE_R",
            Self::Rel => "REL",
            Self::Birel => "BIREL",
            Self::RelU => "REL_U",
            Self::RelD => "REL_D",
            Self::RelL => "REL_L",
            Self::RelR => "REL_R",
            Self::RelB => "REL_B",
            Self::RelIndex => "REL_INDEX",
            Self::UpdateElStyle => "UPDATE_EL_STYLE",
            Self::UpdateRelStyle => "UPDATE_REL_STYLE",
            Self::UpdateLayoutConfig => "UPDATE_LAYOUT_CONFIG",
            Self::EofInStruct => "EOF_IN_STRUCT",
            Self::AttributeEmpty => "ATTRIBUTE_EMPTY",
            Self::Str => "STR",
            Self::StrKey => "STR_KEY",
            Self::StrValue => "STR_VALUE",
            Self::Lbrace => "LBRACE",
            Self::Rbrace => "RBRACE",
            Self::Eof => "EOF",
        }
    }

    /// The token of a keyword, as Pele's `KEYWORDS` maps it.
    pub(crate) fn keyword(word: &str) -> Option<Self> {
        Some(match word {
            "C4Context" => Self::C4Context,
            "C4Container" => Self::C4Container,
            "C4Component" => Self::C4Component,
            "C4Dynamic" => Self::C4Dynamic,
            "C4Deployment" => Self::C4Deployment,
            "Person" => Self::Person,
            "Person_Ext" => Self::PersonExt,
            "System" => Self::System,
            "SystemDb" => Self::SystemDb,
            "SystemQueue" => Self::SystemQueue,
            "System_Ext" => Self::SystemExt,
            "SystemDb_Ext" => Self::SystemExtDb,
            "SystemQueue_Ext" => Self::SystemExtQueue,
            "Container" => Self::Container,
            "ContainerDb" => Self::ContainerDb,
            "ContainerQueue" => Self::ContainerQueue,
            "Container_Ext" => Self::ContainerExt,
            "ContainerDb_Ext" => Self::ContainerExtDb,
            "ContainerQueue_Ext" => Self::ContainerExtQueue,
            "Component" => Self::Component,
            "ComponentDb" => Self::ComponentDb,
            "ComponentQueue" => Self::ComponentQueue,
            "Component_Ext" => Self::ComponentExt,
            "ComponentDb_Ext" => Self::ComponentExtDb,
            "ComponentQueue_Ext" => Self::ComponentExtQueue,
            "Enterprise_Boundary" => Self::EnterpriseBoundary,
            "System_Boundary" => Self::SystemBoundary,
            "Boundary" => Self::Boundary,
            "Container_Boundary" => Self::ContainerBoundary,
            "Deployment_Node" | "Node" => Self::Node,
            "Node_L" => Self::NodeL,
            "Node_R" => Self::NodeR,
            "Rel" => Self::Rel,
            "BiRel" => Self::Birel,
            "Rel_Up" | "Rel_U" => Self::RelU,
            "Rel_Down" | "Rel_D" => Self::RelD,
            "Rel_Left" | "Rel_L" => Self::RelL,
            "Rel_Right" | "Rel_R" => Self::RelR,
            "Rel_Back" => Self::RelB,
            "RelIndex" => Self::RelIndex,
            "UpdateElementStyle" => Self::UpdateElStyle,
            "UpdateRelStyle" => Self::UpdateRelStyle,
            "UpdateLayoutConfig" => Self::UpdateLayoutConfig,
            _ => return None,
        })
    }
}
