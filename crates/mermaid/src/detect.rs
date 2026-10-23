//! Picks the diagram type from preprocessed source with Mermaid's detectors. Port of Pele's
//! `src/detect.ts`.

use crate::util::chars::has;
use crate::util::chars::is_space;
use crate::util::chars::is_word;

fn space(c: char) -> bool {
    is_space(u32::from(c))
}

/// Whether `s` starts with `prefix`, ignoring ASCII case as a non-Unicode `/i` pattern does.
fn starts_ignore_case(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// Whether `\b` holds after the ASCII prefix that ends at `at`.
fn boundary(s: &str, at: usize) -> bool {
    s.get(at..).and_then(|rest| rest.chars().next()).is_none_or(|c| !is_word(u32::from(c)))
}

/// Whether the character at `at` is whitespace or `extra`, or the text ends there.
fn ends_or(s: &str, at: usize, extra: char) -> bool {
    s.get(at..).and_then(|rest| rest.chars().next()).is_none_or(|c| space(c) || c == extra)
}

/// Returns the diagram type, using Pele's type names, or `None` when no detector matches.
/// Detectors run in the order Mermaid registers them, and the first match wins.
pub(crate) fn detect(cleaned: &str) -> Option<&'static str> {
    // Every detector but one anchors at `^\s*`.
    let s = cleaned.trim_start_matches(space);
    let starts = |prefix: &str| s.starts_with(prefix);

    if starts("flowchart-elk") {
        return Some("flowchart");
    }
    if starts("mindmap") {
        return Some("mindmap");
    }
    if starts("architecture") {
        return Some("architecture");
    }
    if starts("agentflow-beta") && boundary(s, 14) {
        return Some("agentflow");
    }
    // Mermaid's pattern anchors only its first alternative, so the others match anywhere.
    if starts("C4Context")
        || ["C4Container", "C4Component", "C4Dynamic", "C4Deployment"].iter().any(|name| has(cleaned, name))
    {
        return Some("c4");
    }

    let plain = [
        ("kanban", "kanban"),
        ("classDiagram", "class"),
        ("erDiagram", "er"),
        ("gantt", "gantt"),
        ("info", "info"),
        ("pie", "pie"),
        ("requirement", "requirement"),
        ("sequenceDiagram", "sequence"),
    ];
    if let Some(&(_, name)) = plain.iter().find(|(prefix, _)| starts(prefix)) {
        return Some(name);
    }
    if starts("swimlane-beta") && boundary(s, 13) {
        return Some("swimlane");
    }

    let plain = [
        ("graph", "flowchart"),
        ("flowchart", "flowchart"),
        ("timeline", "timeline"),
        ("gitGraph", "gitGraph"),
        ("stateDiagram", "state"),
        ("journey", "journey"),
        ("quadrantChart", "quadrantChart"),
        ("sankey", "sankey"),
        ("packet", "packet"),
        ("xychart", "xychart"),
        ("block", "block"),
        ("eventmodeling", "eventmodeling"),
        ("treeView-beta", "treeView"),
        ("radar-beta", "radar"),
    ];
    if let Some(&(_, name)) = plain.iter().find(|(prefix, _)| starts(prefix)) {
        return Some(name);
    }
    // `ishikawa(-beta)?\b` backtracks to `ishikawa\b` when `-beta` is not followed by a boundary.
    if starts_ignore_case(s, "ishikawa") && boundary(s, 8) {
        return Some("ishikawa");
    }
    if starts("treemap") {
        return Some("treemap");
    }
    if starts_ignore_case(s, "railroad-") {
        let rest = s.get(9..).unwrap_or("");
        if ["beta", "ebnf-beta", "abnf-beta", "peg-beta"].iter().any(|tail| starts_ignore_case(rest, tail)) {
            return Some("railroad");
        }
    }
    if starts("venn-beta") {
        return Some("venn");
    }
    if starts_ignore_case(s, "wardley-beta") {
        return Some("wardley");
    }
    if starts("cynefin-beta") && ends_or(s, 12, ':') {
        return Some("cynefin");
    }
    if starts("usecase-beta") && ends_or(s, 12, ' ') {
        return Some("usecase");
    }
    None
}
