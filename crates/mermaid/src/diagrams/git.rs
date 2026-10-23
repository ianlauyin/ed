//! Git graphs: Mermaid's `gitGraph`. Port of Pele's `src/diagrams/git/index.ts`.

pub(crate) mod db;
pub(crate) mod parser;
pub(crate) mod render;

use super::Diagram;
use super::Input;
use super::Rendered;
use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::UrlOptions;
use crate::yaml::Value;

/// The `gitGraph` diagram type.
pub(crate) const DIAGRAM: Diagram = Diagram { keep_comments: false, render: render_input, section: None };

fn render_input(input: &Input<'_>, options: &RenderOptions, _url: &UrlOptions<'_>) -> Result<Rendered, Error> {
    let model = parse(input.source, input.config, input.title)?;
    Ok(render::render(&model, input.config, options))
}

/// The built git graph: Pele's `GitModel`.
pub(crate) use db::GitModel as Model;

/// Parses preprocessed git graph `source` with `config`, as Pele's `gitGraph.parse` does. `title`
/// comes from front matter.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for an operation
/// Mermaid's database rejects.
pub(crate) fn parse(source: &str, config: &Value, title: Option<&str>) -> Result<Model, Error> {
    db::build_git(parser::parse_git(source)?, config, title)
}

/// Serializes `model` as `JSON.stringify` serializes this normalization of Pele's `GitModel`,
/// which the parity tests apply on Pele's side: `head` becomes its commit's id, every `Map`
/// becomes an array of `[key, value]` pairs in insertion order, every object's keys are sorted,
/// and `undefined` fields are dropped.
///
/// ```ts
/// const norm = (v) =>
///   v instanceof Map ? [...v].map(([k, x]) => [k, norm(x)])
///   : Array.isArray(v) ? v.map(norm)
///   : v && typeof v === 'object'
///     ? Object.fromEntries(Object.keys(v).sort().filter((k) => v[k] !== undefined).map((k) => [k, norm(v[k])]))
///     : v;
/// JSON.stringify(norm({ ...model, head: model.head?.id ?? null }));
/// ```
pub(crate) fn to_json(model: &Model) -> String {
    use crate::yaml::push_number;

    let mut out = String::from("{");
    if let Some(text) = &model.acc_descr {
        out.push_str("\"accDescr\":");
        push_json_string(&mut out, text);
        out.push(',');
    }
    if let Some(text) = &model.acc_title {
        out.push_str("\"accTitle\":");
        push_json_string(&mut out, text);
        out.push(',');
    }

    out.push_str("\"branches\":[");
    for (i, (name, head)) in model.branches.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        push_json_string(&mut out, name);
        out.push(',');
        match head {
            Some(id) => push_json_string(&mut out, id),
            None => out.push_str("null"),
        }
        out.push(']');
    }

    out.push_str("],\"commits\":[");
    for (i, commit) in model.commits.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        push_json_string(&mut out, &commit.id);
        out.push_str(",{\"branch\":");
        push_json_string(&mut out, &commit.branch);
        if let Some(custom) = commit.custom_id {
            out.push_str(if custom { ",\"customId\":true" } else { ",\"customId\":false" });
        }
        if let Some(custom) = commit.custom_type {
            out.push_str(",\"customType\":");
            push_number(&mut out, f64::from(custom));
        }
        out.push_str(",\"id\":");
        push_json_string(&mut out, &commit.id);
        out.push_str(",\"message\":");
        push_json_string(&mut out, &commit.message);
        out.push_str(",\"parents\":");
        push_strings(&mut out, &commit.parents);
        out.push_str(",\"seq\":");
        push_number(&mut out, f64::from(commit.seq));
        out.push_str(",\"tags\":");
        push_strings(&mut out, &commit.tags);
        out.push_str(",\"type\":");
        push_number(&mut out, f64::from(commit.r#type));
        out.push_str("}]");
    }

    out.push_str("],\"currentBranch\":");
    push_json_string(&mut out, &model.current_branch);
    out.push_str(",\"direction\":");
    push_json_string(&mut out, model.direction);
    out.push_str(",\"head\":");
    match &model.head {
        Some(id) => push_json_string(&mut out, id),
        None => out.push_str("null"),
    }
    out.push_str(",\"lanes\":");
    push_strings(&mut out, &model.lanes);
    if let Some(title) = &model.title {
        out.push_str(",\"title\":");
        push_json_string(&mut out, title);
    }
    out.push_str(",\"type\":\"gitGraph\",\"warnings\":");
    push_strings(&mut out, &model.warnings);
    out.push('}');
    out
}

/// Appends an array of strings as `JSON.stringify` writes it.
fn push_strings(out: &mut String, items: &[String]) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(out, item);
    }
    out.push(']');
}

/// Appends `text` as `JSON.stringify` quotes it.
fn push_json_string(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let code = u32::from(c) as usize;
                out.push_str("\\u00");
                out.push(char::from(HEX.get(code >> 4).copied().unwrap_or(b'0')));
                out.push(char::from(HEX.get(code & 15).copied().unwrap_or(b'0')));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
