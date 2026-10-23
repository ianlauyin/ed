//! The Cynefin model: the declared domains with their items, and the transitions between them.
//! Port of Pele's `src/diagrams/cynefin/model.ts`.

use super::parser::CynefinAst;
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// A declared domain: Pele's `CynefinDomain`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CynefinDomain {
    /// The item labels.
    pub(crate) items: Vec<String>,
    /// The domain name.
    pub(crate) name: &'static str,
}

/// A transition between two different domains: Pele's `CynefinTransition`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CynefinTransition {
    /// The domain it leaves.
    pub(crate) from: &'static str,
    /// The label, when it is not empty.
    pub(crate) label: Option<String>,
    /// The domain it enters.
    pub(crate) to: &'static str,
}

/// A Cynefin diagram ready to draw: Pele's `CynefinModel`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CynefinModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// The domains the source declares, in first-declared order. All five are drawn either way.
    pub(crate) domains: Vec<CynefinDomain>,
    /// The title.
    pub(crate) title: Option<String>,
    /// Transitions in authored order.
    pub(crate) transitions: Vec<CynefinTransition>,
}

impl CynefinModel {
    /// The items declared for domain `name`.
    pub(crate) fn items(&self, name: &str) -> &[String] {
        self.domains
            .iter()
            .find(|domain| domain.name == name)
            .map_or(&[], |domain| &domain.items)
    }
}

/// `text.replace(/\n\s+/g, '\n')`.
fn dedent_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
        }
    }
    out
}

/// Builds the model from `ast`, as Pele's `buildCynefin` does. `title` comes from front matter.
pub(crate) fn build_cynefin(ast: CynefinAst, title: Option<&str>) -> CynefinModel {
    // A later block for a domain replaces an earlier one, which keeps its place.
    let mut domains: Vec<CynefinDomain> = Vec::new();
    for block in ast.domains {
        let domain = CynefinDomain {
            items: block.items,
            name: block.domain,
        };
        match domains.iter_mut().find(|known| known.name == domain.name) {
            Some(known) => *known = domain,
            None => domains.push(domain),
        }
    }
    let transitions = ast
        .transitions
        .into_iter()
        .filter(|transition| transition.from != transition.to)
        .map(|transition| CynefinTransition {
            from: transition.from,
            label: transition.label.filter(|label| !label.is_empty()),
            to: transition.to,
        })
        .collect();

    CynefinModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&text).to_owned()),
        domains,
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
        transitions,
    }
}
