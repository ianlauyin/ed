//! Token kinds of the requirement lexer, the names Mermaid's parser reports for them, and the
//! keywords. Port of Pele's `src/diagrams/requirement/tokens.ts`.

/// A token kind. Variant order mirrors Pele's `T` enum, indexes [`NAMES`], and gives the ranges
/// the parser tests, so it is observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum T {
    /// Pele's `T.END`.
    End,
    /// Pele's `T.ERROR`.
    Error,
    /// Pele's `T.title`.
    Title,
    /// Pele's `T.acc_title`.
    AccTitle,
    /// Pele's `T.acc_title_value`.
    AccTitleValue,
    /// Pele's `T.acc_descr`.
    AccDescr,
    /// Pele's `T.acc_descr_value`.
    AccDescrValue,
    /// Pele's `T.acc_descr_multiline_value`.
    AccDescrMultilineValue,
    /// Pele's `T.direction_tb`.
    DirectionTb,
    /// Pele's `T.direction_bt`.
    DirectionBt,
    /// Pele's `T.direction_rl`.
    DirectionRl,
    /// Pele's `T.direction_lr`.
    DirectionLr,
    /// Pele's `T.NEWLINE`.
    Newline,
    /// Pele's `T.EOF`.
    Eof,
    /// Pele's `T.RD`.
    Rd,
    /// Pele's `T.STRUCT_START`.
    StructStart,
    /// Pele's `T.STRUCT_STOP`.
    StructStop,
    /// Pele's `T.STYLE_SEPARATOR`.
    StyleSeparator,
    /// Pele's `T.COLONSEP`.
    Colonsep,
    /// Pele's `T.ID`.
    Id,
    /// Pele's `T.TEXT`.
    Text,
    /// Pele's `T.RISK`.
    Risk,
    /// Pele's `T.VERIFYMTHD`.
    Verifymthd,
    /// Pele's `T.REQUIREMENT`.
    Requirement,
    /// Pele's `T.FUNCTIONAL_REQUIREMENT`.
    FunctionalRequirement,
    /// Pele's `T.INTERFACE_REQUIREMENT`.
    InterfaceRequirement,
    /// Pele's `T.PERFORMANCE_REQUIREMENT`.
    PerformanceRequirement,
    /// Pele's `T.PHYSICAL_REQUIREMENT`.
    PhysicalRequirement,
    /// Pele's `T.DESIGN_CONSTRAINT`.
    DesignConstraint,
    /// Pele's `T.LOW_RISK`.
    LowRisk,
    /// Pele's `T.MED_RISK`.
    MedRisk,
    /// Pele's `T.HIGH_RISK`.
    HighRisk,
    /// Pele's `T.VERIFY_ANALYSIS`.
    VerifyAnalysis,
    /// Pele's `T.VERIFY_DEMONSTRATION`.
    VerifyDemonstration,
    /// Pele's `T.VERIFY_INSPECTION`.
    VerifyInspection,
    /// Pele's `T.VERIFY_TEST`.
    VerifyTest,
    /// Pele's `T.ELEMENT`.
    Element,
    /// Pele's `T.CONTAINS`.
    Contains,
    /// Pele's `T.COPIES`.
    Copies,
    /// Pele's `T.DERIVES`.
    Derives,
    /// Pele's `T.SATISFIES`.
    Satisfies,
    /// Pele's `T.VERIFIES`.
    Verifies,
    /// Pele's `T.REFINES`.
    Refines,
    /// Pele's `T.TRACES`.
    Traces,
    /// Pele's `T.TYPE`.
    Type,
    /// Pele's `T.DOCREF`.
    Docref,
    /// Pele's `T.STYLE`.
    Style,
    /// Pele's `T.CLASSDEF`.
    Classdef,
    /// Pele's `T.CLASS`.
    Class,
    /// Pele's `T.ALPHA`.
    Alpha,
    /// Pele's `T.COLON`.
    Colon,
    /// Pele's `T.SEMICOLON`.
    Semicolon,
    /// Pele's `T.PERCENT`.
    Percent,
    /// Pele's `T.MINUS`.
    Minus,
    /// Pele's `T.BRKT`.
    Brkt,
    /// Pele's `T.END_ARROW_L`.
    EndArrowL,
    /// Pele's `T.END_ARROW_R`.
    EndArrowR,
    /// Pele's `T.LINE`.
    Line,
    /// Pele's `T.qString`.
    QString,
    /// Pele's `T.unqString`.
    UnqString,
    /// Pele's `T.COMMA`.
    Comma,
}

/// Every kind in order, so a number maps back to its kind.
const ALL: [T; 61] = [
    T::End,
    T::Error,
    T::Title,
    T::AccTitle,
    T::AccTitleValue,
    T::AccDescr,
    T::AccDescrValue,
    T::AccDescrMultilineValue,
    T::DirectionTb,
    T::DirectionBt,
    T::DirectionRl,
    T::DirectionLr,
    T::Newline,
    T::Eof,
    T::Rd,
    T::StructStart,
    T::StructStop,
    T::StyleSeparator,
    T::Colonsep,
    T::Id,
    T::Text,
    T::Risk,
    T::Verifymthd,
    T::Requirement,
    T::FunctionalRequirement,
    T::InterfaceRequirement,
    T::PerformanceRequirement,
    T::PhysicalRequirement,
    T::DesignConstraint,
    T::LowRisk,
    T::MedRisk,
    T::HighRisk,
    T::VerifyAnalysis,
    T::VerifyDemonstration,
    T::VerifyInspection,
    T::VerifyTest,
    T::Element,
    T::Contains,
    T::Copies,
    T::Derives,
    T::Satisfies,
    T::Verifies,
    T::Refines,
    T::Traces,
    T::Type,
    T::Docref,
    T::Style,
    T::Classdef,
    T::Class,
    T::Alpha,
    T::Colon,
    T::Semicolon,
    T::Percent,
    T::Minus,
    T::Brkt,
    T::EndArrowL,
    T::EndArrowR,
    T::Line,
    T::QString,
    T::UnqString,
    T::Comma,
];

/// The names Mermaid's parser reports for each token kind, indexed by [`T`].
pub(crate) const NAMES: [&str; 61] = [
    "$end",
    "INVALID",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "NEWLINE",
    "EOF",
    "RD",
    "STRUCT_START",
    "STRUCT_STOP",
    "STYLE_SEPARATOR",
    "COLONSEP",
    "ID",
    "TEXT",
    "RISK",
    "VERIFYMTHD",
    "REQUIREMENT",
    "FUNCTIONAL_REQUIREMENT",
    "INTERFACE_REQUIREMENT",
    "PERFORMANCE_REQUIREMENT",
    "PHYSICAL_REQUIREMENT",
    "DESIGN_CONSTRAINT",
    "LOW_RISK",
    "MED_RISK",
    "HIGH_RISK",
    "VERIFY_ANALYSIS",
    "VERIFY_DEMONSTRATION",
    "VERIFY_INSPECTION",
    "VERIFY_TEST",
    "ELEMENT",
    "CONTAINS",
    "COPIES",
    "DERIVES",
    "SATISFIES",
    "VERIFIES",
    "REFINES",
    "TRACES",
    "TYPE",
    "DOCREF",
    "STYLE",
    "CLASSDEF",
    "CLASS",
    "ALPHA",
    "COLON",
    "SEMICOLON",
    "PERCENT",
    "MINUS",
    "BRKT",
    "END_ARROW_L",
    "END_ARROW_R",
    "LINE",
    "qString",
    "unqString",
    "COMMA",
];

impl T {
    /// The name Mermaid's parser reports for this kind.
    pub(crate) fn name(self) -> &'static str {
        NAMES.get(self as usize).copied().unwrap_or("")
    }

    /// The kind `offset` places after this one, as Pele adds to a `T` value.
    pub(crate) fn offset(self, offset: usize) -> T {
        ALL.get(self as usize + offset).copied().unwrap_or(T::End)
    }

    /// The kinds from `self` to `to`, inclusive.
    pub(crate) fn through(self, to: T) -> impl Iterator<Item = T> {
        ALL.into_iter().filter(move |&t| t >= self && t <= to)
    }
}

/// The keyword a lowercased word names. Every keyword is matched without regard to case and must
/// end at a word boundary.
pub(crate) fn keyword(word: &str) -> Option<T> {
    Some(match word {
        "requirementdiagram" => T::Rd,
        "id" => T::Id,
        "text" => T::Text,
        "risk" => T::Risk,
        "verifymethod" => T::Verifymthd,
        "requirement" => T::Requirement,
        "functionalrequirement" => T::FunctionalRequirement,
        "interfacerequirement" => T::InterfaceRequirement,
        "performancerequirement" => T::PerformanceRequirement,
        "physicalrequirement" => T::PhysicalRequirement,
        "designconstraint" => T::DesignConstraint,
        "low" => T::LowRisk,
        "medium" => T::MedRisk,
        "high" => T::HighRisk,
        "analysis" => T::VerifyAnalysis,
        "demonstration" => T::VerifyDemonstration,
        "inspection" => T::VerifyInspection,
        "test" => T::VerifyTest,
        "element" => T::Element,
        "contains" => T::Contains,
        "copies" => T::Copies,
        "derives" => T::Derives,
        "satisfies" => T::Satisfies,
        "verifies" => T::Verifies,
        "refines" => T::Refines,
        "traces" => T::Traces,
        "type" => T::Type,
        "docref" => T::Docref,
        "style" => T::Style,
        "classdef" => T::Classdef,
        "class" => T::Class,
        _ => return None,
    })
}
