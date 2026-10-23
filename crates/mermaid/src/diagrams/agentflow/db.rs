//! The agentflow model builder, equivalent to Mermaid's `AgentFlowDB`. Port of Pele's
//! `src/diagrams/agentflow/db.ts`.
//!
//! Pele keeps nodes, connectors, classes, and tooltips in `Map`s. Here each is a `Vec` in
//! insertion order with a `BTreeMap` index for lookups, so iteration order never depends on
//! hashing. A connector is a node, so the connector list holds node ids.

use std::collections::{BTreeMap, BTreeSet};

use super::shapes::{is_valid_shape, resolve_shape_alias};
use super::types::{
    AgentEdge, AgentNode, AgentSubgraph, Diagnostic, DocItem, ElementMapping, ElementPosition,
    FlowClassDef, FlowText, LabelType, LinkInfo, Loc, Metadata,
};
use crate::error::{Code, Error, push_u32};
use crate::util::chars::is_space;
use crate::util::url;
use crate::yaml::{self, Value};

/// Options Pele's agentflow entry passes to `AgentflowDb`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    /// The `direction` config value, which flows inherit when `inherit_dir` is set and the
    /// diagram has none.
    pub(crate) direction: Option<String>,
    /// Whether flows without a `direction` inherit the diagram's.
    pub(crate) inherit_dir: bool,
    /// Lines of front matter before the text the parser sees, added to every reported line.
    pub(crate) line_offset: u32,
    /// The most edges a diagram may have.
    pub(crate) max_edges: Option<usize>,
}

/// The parsed agentflow: Pele's `AgentflowDb`, which implements `AgentflowModel`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AgentflowDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Class definitions, in insertion order.
    pub(crate) classes: Vec<FlowClassDef>,
    /// Ids of the nodes `connector` statements made, in insertion order.
    pub(crate) connectors: Vec<String>,
    /// Styles from `linkStyle default`.
    pub(crate) default_edge_style: Option<Vec<String>>,
    /// The curve from `linkStyle default interpolate`.
    pub(crate) default_interpolate: Option<String>,
    /// Shape and containment findings, in the order they were made.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// The diagram direction.
    pub(crate) direction: Option<String>,
    /// Edges, in insertion order.
    pub(crate) edges: Vec<AgentEdge>,
    /// Where each statement sits in the source, in the order the statements were read.
    pub(crate) mappings: Vec<ElementMapping>,
    /// Nodes and connectors, in insertion order.
    pub(crate) nodes: Vec<AgentNode>,
    /// Flows, in the order they close.
    pub(crate) subgraphs: Vec<AgentSubgraph>,
    /// The front matter title.
    pub(crate) title: Option<String>,
    /// Tooltips by node id, in insertion order.
    pub(crate) tooltips: Vec<(String, String)>,

    bare_mappings: BTreeSet<usize>,
    class_index: BTreeMap<String, usize>,
    connector_index: BTreeSet<String>,
    edge_by_id: BTreeMap<String, usize>,
    first_mapping: BTreeMap<String, usize>,
    global_nodes: BTreeSet<String>,
    members: BTreeSet<String>,
    node_index: BTreeMap<String, usize>,
    options: Options,
    pair_count: BTreeMap<(String, String), usize>,
    reported: BTreeMap<String, Vec<usize>>,
    sub_count: usize,
    subgraph_lookup: BTreeMap<String, usize>,
    tooltip_index: BTreeMap<String, usize>,
}

/// Metadata keys that only change how an element looks, left out of the semantic model.
#[cfg(feature = "model")]
pub(crate) const PRESENTATION_KEYS: [&str; 11] = [
    "shape",
    "view",
    "icon",
    "img",
    "form",
    "pos",
    "w",
    "h",
    "class",
    "style",
    "labelType",
];

/// Shapes that define a tool.
const TOOL_SHAPES: [&str; 5] = [
    "subroutine",
    "subprocess",
    "subproc",
    "framed-rectangle",
    "tool",
];

/// Whether `vertex` defines a tool.
pub(crate) fn is_tool_definition(vertex: &AgentNode) -> bool {
    vertex
        .r#type
        .as_deref()
        .is_some_and(|r#type| TOOL_SHAPES.contains(&r#type))
}

/// Pele's `labelType`: a known label type name, or Markdown.
fn label_type(r#type: Option<&Value>) -> LabelType {
    match r#type {
        Some(Value::String(name)) if name == "string" => LabelType::String,
        Some(Value::String(name)) if name == "text" => LabelType::Text,
        _ => LabelType::Markdown,
    }
}

/// Strips one pair of surrounding double quotes.
fn unquote(text: &str) -> &str {
    if text.starts_with('"') && text.ends_with('"') {
        text.get(1..text.len().saturating_sub(1)).unwrap_or("")
    } else {
        text
    }
}

/// JavaScript's `String.prototype.trim`, which strips what `\s` matches.
pub(crate) fn js_trim(text: &str) -> &str {
    text.trim_matches(|c| is_space(u32::from(c)))
}

/// Drops `__proto__`, `constructor`, and `prototype` keys at every level, so that a consumer
/// merging metadata into its own objects cannot be made to pollute a prototype. YAML values nest
/// at most 64 deep, which bounds the recursion.
fn strip_prototype_keys(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(strip_prototype_keys).collect()),
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .filter(|(key, _)| {
                    !matches!(key.as_str(), "__proto__" | "constructor" | "prototype")
                })
                .map(|(key, value)| (key, strip_prototype_keys(value)))
                .collect(),
        ),
        other => other,
    }
}

/// Adds what a metadata block holds to an element's metadata, as `Object.assign` does. Mermaid
/// spreads the parsed YAML whatever it is, so a list adds its indexes as keys and a string its
/// characters.
fn merge(into: &mut Option<Metadata>, doc: &Value) {
    let target = into.get_or_insert_with(Vec::new);
    match doc {
        Value::Object(entries) => {
            for (key, value) in entries {
                yaml::set(target, key.clone(), value.clone());
            }
        }
        Value::Array(items) => {
            for (at, item) in items.iter().enumerate() {
                yaml::set(target, index_key(at), item.clone());
            }
        }
        Value::String(text) => {
            // JavaScript indexes a string by UTF-16 unit. A character outside the Basic
            // Multilingual Plane keeps its first index here, where JavaScript splits it in two.
            let mut at = 0;
            for c in text.chars() {
                yaml::set(target, index_key(at), Value::String(String::from(c)));
                at += c.len_utf16();
            }
        }
        Value::Bool(_) | Value::Null | Value::Number(_) => {}
    }
}

/// An array index as a property key.
fn index_key(at: usize) -> String {
    let mut key = String::new();
    push_u32(&mut key, u32::try_from(at).unwrap_or(u32::MAX));
    key
}

/// Appends `value` as `JSON.stringify` writes it.
pub(crate) fn push_json(out: &mut String, value: &Value) {
    match value {
        Value::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                push_json(out, item);
            }
            out.push(']');
        }
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
        Value::Number(value) if !value.is_finite() => out.push_str("null"),
        Value::Number(value) => yaml::push_number(out, *value),
        Value::Object(entries) => {
            out.push('{');
            for (at, (key, value)) in entries.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                push_json_string(out, key);
                out.push(':');
                push_json(out, value);
            }
            out.push('}');
        }
        Value::String(text) => push_json_string(out, text),
    }
}

/// Appends `text` as a JSON string literal, as `JSON.stringify` escapes it.
pub(crate) fn push_json_string(out: &mut String, text: &str) {
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
                let code = u32::from(c);
                out.push_str("\\u00");
                for nibble in [code >> 4, code & 15] {
                    let digit = HEX.get(nibble as usize).copied().unwrap_or(b'0');
                    out.push(char::from(digit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Whether a line of a block scalar header ends the line: `/:\s*[>|][\d+-]*\s*$/`.
fn opens_block_scalar(code: &str) -> bool {
    let trimmed = code.trim_end_matches(|c| is_space(u32::from(c)));
    let body = trimmed.trim_end_matches(|c: char| c.is_ascii_digit() || c == '+' || c == '-');
    let Some(rest) = body.strip_suffix('>').or_else(|| body.strip_suffix('|')) else {
        return false;
    };
    rest.trim_end_matches(|c| is_space(u32::from(c)))
        .ends_with(':')
}

/// Removes the comma that ends a line of a multi-line metadata body, which block YAML does not
/// allow. Commas inside quoted scalars, flow collections, and block scalars are content and
/// stay.
fn strip_line_trailing_commas(body: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    let mut flow_depth = 0usize;
    let mut block_scalar_indent: Option<usize> = None;
    let mut out: Vec<String> = Vec::new();

    for line in body.split('\n') {
        let indent = line.encode_utf16().count()
            - line
                .trim_start_matches(|c| is_space(u32::from(c)))
                .encode_utf16()
                .count();
        if let Some(scalar) = block_scalar_indent {
            if js_trim(line).is_empty() || indent > scalar {
                out.push(line.to_owned());
                continue;
            }
            block_scalar_indent = None;
        }

        let mut comment_start = None;
        let mut chars = line.char_indices();
        while let Some((at, ch)) = chars.next() {
            let previous = line.get(..at).and_then(|before| before.chars().next_back());
            if in_double {
                if ch == '\\' {
                    chars.next();
                } else if ch == '"' {
                    in_double = false;
                }
            } else if in_single {
                if ch == '\'' {
                    in_single = false;
                }
            } else if ch == '"' {
                in_double = true;
            } else if ch == '\'' {
                in_single = true;
            } else if ch == '#' && (at == 0 || matches!(previous, Some(' ' | '\t'))) {
                comment_start = Some(at);
                break;
            } else if ch == '[' || ch == '{' {
                flow_depth += 1;
            } else if ch == ']' || ch == '}' {
                flow_depth = flow_depth.saturating_sub(1);
            }
        }
        if in_single || in_double || flow_depth > 0 {
            out.push(line.to_owned());
            continue;
        }

        let (code, comment) = match comment_start {
            Some(at) => (line.get(..at).unwrap_or(""), line.get(at..).unwrap_or("")),
            None => (line, ""),
        };
        if opens_block_scalar(code) {
            block_scalar_indent = Some(indent);
            out.push(line.to_owned());
            continue;
        }

        // `code.replace(/,([\t ]*)$/, '$1')`.
        let kept = code.trim_end_matches([' ', '\t']);
        let mut text = match kept.strip_suffix(',') {
            Some(before) => {
                let mut text = before.to_owned();
                text.push_str(code.get(kept.len()..).unwrap_or(""));
                text
            }
            None => code.to_owned(),
        };
        text.push_str(comment);
        out.push(text);
    }
    out.join("\n")
}

/// What an arrow means, for the arrows that mean something.
fn edge_semantic(r#type: &str, stroke: &str) -> Option<&'static str> {
    match (r#type, stroke) {
        ("arrow_point", "normal") => Some("sequence"),
        ("arrow_cross", "normal") => Some("failure"),
        ("arrow_open", "dotted") => Some("reference"),
        _ => None,
    }
}

/// Pele's `destructEndLink`. Link tokens are ASCII once trimmed, so byte slicing is safe.
fn destruct_end_link(raw: &str) -> (&'static str, &'static str, i32) {
    let s = js_trim(raw);
    let mut line = s.get(..s.len().saturating_sub(1)).unwrap_or("");
    let r#type = match s.as_bytes().last() {
        Some(b'x') => "arrow_cross",
        Some(b'>') => "arrow_point",
        Some(b'-' | b'.') => {
            line = s;
            "arrow_open"
        }
        _ => "arrow_open",
    };

    let mut stroke = "normal";
    let mut length = i32::try_from(line.encode_utf16().count()).unwrap_or(i32::MAX) - 1;
    let dots = line.bytes().filter(|&b| b == b'.').count();
    if dots > 0 {
        stroke = "dotted";
        length = i32::try_from(dots).unwrap_or(i32::MAX);
    }
    (r#type, stroke, length)
}

/// A property of a YAML mapping.
fn get<'v>(doc: &'v [(String, Value)], key: &str) -> Option<&'v Value> {
    doc.iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

/// Whether lowercasing `text` would change it, as `text !== text.toLowerCase()` tests.
fn has_upper(text: &str) -> bool {
    text.chars().any(|c| {
        let mut lower = c.to_lowercase();
        lower.next() != Some(c) || lower.next().is_some()
    })
}

/// JavaScript's `String(value)` for a YAML value.
fn js_string(value: &Value) -> String {
    let mut out = String::new();
    value.push_js_string(&mut out);
    out
}

/// An agentflow error without a source position.
#[cold]
fn agentflow_error(code: Code, message: String) -> Error {
    let mut error = Error::new(code, message);
    error.r#type = Some("agentflow");
    error
}

impl AgentflowDb {
    /// Creates an empty model.
    pub(crate) fn new(options: Options) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut AgentNode> {
        let index = *self.node_index.get(id)?;
        self.nodes.get_mut(index)
    }

    /// The node with `id`.
    pub(crate) fn node(&self, id: &str) -> Option<&AgentNode> {
        self.node_index
            .get(id)
            .and_then(|&index| self.nodes.get(index))
    }

    /// Whether a `connector` statement made the node `id`.
    pub(crate) fn is_connector(&self, id: &str) -> bool {
        self.connector_index.contains(id)
    }

    /// The tooltip of node `id`.
    pub(crate) fn tooltip(&self, id: &str) -> Option<&str> {
        self.tooltip_index
            .get(id)
            .and_then(|&index| self.tooltips.get(index))
            .map(|(_, text)| text.as_str())
    }

    /// The class definition named `id`.
    pub(crate) fn class(&self, id: &str) -> Option<&FlowClassDef> {
        self.class_index
            .get(id)
            .and_then(|&index| self.classes.get(index))
    }

    /// The flow with `id`.
    pub(crate) fn subgraph(&self, id: &str) -> Option<&AgentSubgraph> {
        self.subgraph_lookup
            .get(id)
            .and_then(|&index| self.subgraphs.get(index))
    }

    /// The line a span starts on, with the front matter offset.
    fn line_of(&self, loc: Option<Loc>) -> u32 {
        loc.map_or(0, |loc| loc.first_line + self.options.line_offset)
    }

    /// Reads the text of a metadata block. Block YAML rejects the comma-separated style of the
    /// single-line form, so a multi-line body is read once more without its trailing commas.
    fn metadata(&self, id: &str, source: &str, loc: Option<Loc>) -> Result<Value, Error> {
        let multiline = source.contains('\n');
        let mut text = String::with_capacity(source.len() + 4);
        if multiline {
            text.push_str(source);
            text.push('\n');
        } else {
            text.push_str("{\n");
            text.push_str(source);
            text.push_str("\n}");
        }

        let parsed = match yaml::parse(&text) {
            Ok(value) => value,
            Err(message) => {
                let stripped = if multiline {
                    strip_line_trailing_commas(source)
                } else {
                    source.to_owned()
                };
                let retried = (stripped != source)
                    .then(|| {
                        let mut text = stripped;
                        text.push('\n');
                        yaml::parse(&text).ok()
                    })
                    .flatten();
                let Some(retried) = retried else {
                    let mut text = String::from("Metadata of \"");
                    text.push_str(id);
                    text.push_str("\" is not valid YAML. ");
                    text.push_str(&message);
                    if let Some(loc) = loc {
                        text.push_str(" (");
                        push_u32(&mut text, loc.first_line + self.options.line_offset);
                        text.push(':');
                        push_u32(&mut text, loc.first_column + 1);
                        text.push(')');
                    }
                    let mut error = agentflow_error(Code::Syntax, text);
                    error.line = self.line_of(loc);
                    error.column = loc.map_or(0, |loc| loc.first_column + 1);
                    return Err(error);
                };
                retried
            }
        };
        let parsed = if parsed == Value::Null {
            Value::Object(Vec::new())
        } else {
            parsed
        };
        Ok(strip_prototype_keys(parsed))
    }

    #[cold]
    fn shape_error(&self, message: String, loc: Option<Loc>) -> Error {
        let mut error = agentflow_error(Code::Semantic, message);
        error.line = self.line_of(loc);
        error.column = loc.map_or(0, |loc| loc.first_column + 1);
        error
    }

    /// Adds or updates a node, or adds metadata to the flow, connector, or edge `id` names.
    /// `metadata` is the text of a `@{ ... }` block and its span. Pele's `classes` and `dir`
    /// parameters are never passed by the parser, so they are left out.
    pub(crate) fn add_vertex(
        &mut self,
        id: &str,
        text: Option<FlowText>,
        r#type: Option<&str>,
        style: Option<Vec<String>>,
        props: Option<(String, String)>,
        metadata: Option<(&str, Option<Loc>)>,
    ) -> Result<(), Error> {
        if js_trim(id).is_empty() {
            return Ok(());
        }

        let mut doc = None;
        let mut authored_shape = None;
        let mut loc = None;
        if let Some((source, at)) = metadata {
            loc = at;
            let mut value = self.metadata(id, source, at)?;
            if let Value::Object(entries) = &mut value
                && let Some((_, Value::String(shape))) =
                    entries.iter_mut().find(|(key, _)| key == "shape")
            {
                authored_shape = Some(shape.clone());
                let resolved = resolve_shape_alias(shape).to_owned();
                *shape = resolved;
            }
            doc = Some(value);
        }
        let doc = doc.filter(Value::truthy);

        if let (Some(&index), Some(doc)) = (self.subgraph_lookup.get(id), &doc) {
            if let Some(subgraph) = self.subgraphs.get_mut(index) {
                merge(&mut subgraph.metadata, doc);
            }
            return Ok(());
        }

        if let Some(doc) = &doc
            && self.connector_index.contains(id)
        {
            if let Some(connector) = self.node_mut(id) {
                merge(&mut connector.metadata, doc);
            }
            return Ok(());
        }

        // Mermaid reserves this id and creates no node for it.
        if id == "connectors" {
            return Ok(());
        }

        if let Some(&index) = self.edge_by_id.get(id) {
            if let Some(edge) = self.edges.get_mut(index) {
                if let Some(Value::Object(fields)) = &doc {
                    if let Some(value) = get(fields, "animate") {
                        edge.animate = Some(value.clone());
                    }
                    if let Some(value) = get(fields, "animation") {
                        edge.animation = Some(value.clone());
                    }
                    if let Some(value) = get(fields, "curve") {
                        edge.interpolate = Some(value.clone());
                    }
                }
                if let Some(doc) = &doc {
                    merge(&mut edge.metadata, doc);
                }
            }
            return Ok(());
        }

        if !self.node_index.contains_key(id) {
            self.node_index.insert(id.to_owned(), self.nodes.len());
            self.nodes.push(AgentNode {
                classes: Vec::new(),
                dir: None,
                id: id.to_owned(),
                is_connector: false,
                label_type: LabelType::Text,
                link: None,
                link_target: None,
                metadata: None,
                props: None,
                styles: Vec::new(),
                text: None,
                r#type: None,
            });
        }
        let Some(&index) = self.node_index.get(id) else {
            return Ok(());
        };
        let Some(vertex) = self.nodes.get_mut(index) else {
            return Ok(());
        };

        if let Some(text) = text {
            vertex.label_type = text.r#type;
            vertex.text = Some(unquote(js_trim(&text.text)).to_owned());
        } else if vertex.text.is_none() {
            vertex.text = Some(id.to_owned());
        }
        if let Some(r#type) = r#type {
            vertex.r#type = Some(r#type.to_owned());
        }
        if let Some(style) = style {
            vertex.styles.extend(style);
        }
        match &mut vertex.props {
            None => vertex.props = Some(props.into_iter().collect()),
            Some(existing) => {
                // Without an own `__proto__`, `Object.assign` sets the prototype instead.
                if let Some((key, value)) = props {
                    match existing.iter_mut().find(|(own, _)| *own == key) {
                        Some(entry) => entry.1 = value,
                        None if key == "__proto__" => {}
                        None => existing.push((key, value)),
                    }
                }
            }
        }

        let Some(doc) = doc else {
            return Ok(());
        };
        merge(&mut vertex.metadata, &doc);
        let Value::Object(fields) = &doc else {
            return Ok(());
        };

        if let Some(shape) = get(fields, "shape").filter(|value| value.truthy()) {
            let Some(authored) = authored_shape.as_deref() else {
                let mut message = String::from("No such shape: ");
                push_json(&mut message, shape);
                message.push('.');
                return Err(self.shape_error(message, loc));
            };
            if has_upper(authored) || authored.contains('_') {
                let mut message = String::from("No such shape: ");
                message.push_str(authored);
                message.push_str(". Shape names should be lowercase.");
                return Err(self.shape_error(message, loc));
            }
            let shape = js_string(shape);
            if !is_valid_shape(&shape) {
                let mut message = String::from("No such shape: ");
                message.push_str(&shape);
                message.push('.');
                return Err(self.shape_error(message, loc));
            }
            let Some(vertex) = self.nodes.get_mut(index) else {
                return Ok(());
            };
            vertex.r#type = Some(shape);
        }

        if let Some(label) = get(fields, "label").filter(|value| value.truthy()) {
            let Some(vertex) = self.nodes.get_mut(index) else {
                return Ok(());
            };
            vertex.text = Some(js_string(label));
            vertex.label_type = label_type(get(fields, "labelType"));
        }
        Ok(())
    }

    /// Adds one edge. `id` is the user-defined id, without its `@`.
    fn add_single_link(
        &mut self,
        start: &str,
        end: &str,
        info: &LinkInfo,
        id: Option<&str>,
    ) -> Result<(), Error> {
        let mut edge = AgentEdge {
            animate: None,
            animation: None,
            classes: Vec::new(),
            edge_semantic: info.edge_semantic,
            end: end.to_owned(),
            id: String::new(),
            interpolate: self.default_interpolate.clone().map(Value::String),
            is_user_defined_id: false,
            label_type: LabelType::Text,
            length: info.length.map(|length| length.min(10)),
            metadata: None,
            start: start.to_owned(),
            stroke: info.stroke,
            style: None,
            text: String::new(),
            r#type: info.r#type,
        };
        if let Some(text) = &info.text {
            unquote(js_trim(&text.text)).clone_into(&mut edge.text);
            edge.label_type = text.r#type;
        }

        let pair = (start.to_owned(), end.to_owned());
        let existing = self.pair_count.get(&pair).copied().unwrap_or(0);
        match id {
            Some(id) if !id.is_empty() && !self.edge_by_id.contains_key(id) => {
                edge.is_user_defined_id = true;
                id.clone_into(&mut edge.id);
            }
            _ => {
                let mut generated = String::from("L_");
                generated.push_str(start);
                generated.push('_');
                generated.push_str(end);
                generated.push('_');
                let count = if existing == 0 { 0 } else { existing + 1 };
                push_u32(&mut generated, u32::try_from(count).unwrap_or(u32::MAX));
                edge.id = generated;
            }
        }

        if let Some(max) = self.options.max_edges
            && self.edges.len() >= max
        {
            let mut text = String::from("Edge limit exceeded. ");
            push_u32(
                &mut text,
                u32::try_from(self.edges.len()).unwrap_or(u32::MAX),
            );
            text.push_str(" edges found, but the limit is ");
            push_u32(&mut text, u32::try_from(max).unwrap_or(u32::MAX));
            text.push('.');
            return Err(agentflow_error(Code::Limit, text));
        }

        if !self.edge_by_id.contains_key(&edge.id) {
            self.edge_by_id.insert(edge.id.clone(), self.edges.len());
        }
        self.edges.push(edge);
        *self.pair_count.entry(pair).or_insert(0) += 1;
        Ok(())
    }

    /// Adds an edge from every start to every end. Only the edge from the last start to the
    /// first end takes the user-defined id.
    pub(crate) fn add_link(
        &mut self,
        starts: &[String],
        ends: &[String],
        info: &LinkInfo,
    ) -> Result<(), Error> {
        let id = info.id.as_deref().map(|id| id.replacen('@', "", 1));
        let last_start = starts.last();
        let first_end = ends.first();
        for start in starts {
            for end in ends {
                let own = Some(start) == last_start && Some(end) == first_end;
                self.add_single_link(start, end, info, if own { id.as_deref() } else { None })?;
            }
        }
        Ok(())
    }

    /// Sets the curve of the edges at `positions`, or the default curve.
    pub(crate) fn update_link_interpolate(
        &mut self,
        positions: &[String],
        interpolate: &str,
    ) -> Result<(), Error> {
        for pos in positions {
            if pos == "default" {
                self.default_interpolate = Some(interpolate.to_owned());
            } else {
                self.edge_at(pos)?.interpolate = Some(Value::String(interpolate.to_owned()));
            }
        }
        Ok(())
    }

    /// Sets the styles of the edges at `positions`, or the default styles.
    pub(crate) fn update_link(
        &mut self,
        positions: &[String],
        mut style: Vec<String>,
    ) -> Result<(), Error> {
        // Pele assigns one array to every position and appends `fill:none` to it at the first
        // edge, so every position of the statement sees the appended value.
        if positions.iter().any(|pos| pos != "default")
            && !style.is_empty()
            && !style.iter().any(|s| s.starts_with("fill"))
        {
            style.push("fill:none".to_owned());
        }
        for pos in positions {
            if pos == "default" {
                self.default_edge_style = Some(style.clone());
            } else {
                self.edge_at(pos)?.style = Some(style.clone());
            }
        }
        Ok(())
    }

    /// The edge at a `linkStyle` index. Mermaid checks the bounds only for a numeric index and
    /// crashes on the strings its own parser passes, so Pele reports both.
    fn edge_at(&mut self, pos: &str) -> Result<&mut AgentEdge, Error> {
        let count = self.edges.len();
        let index = pos.parse::<usize>().ok().filter(|&index| index < count);
        if let Some(edge) = index.and_then(|index| self.edges.get_mut(index)) {
            return Ok(edge);
        }

        let mut text = String::from("The index ");
        text.push_str(pos);
        text.push_str(
            " for linkStyle is out of bounds. Valid indices for linkStyle are between 0 and ",
        );
        if count == 0 {
            text.push_str("-1");
        } else {
            push_u32(&mut text, u32::try_from(count - 1).unwrap_or(u32::MAX));
        }
        text.push_str(". (Help: Ensure that the index is within the range of existing edges.)");
        Err(agentflow_error(Code::Semantic, text))
    }

    /// Defines classes from `classDef`.
    pub(crate) fn add_class(&mut self, ids: &str, styles: &[String]) {
        let joined = styles.join(",");
        let escaped = replace_all(&joined, "\\,", "§§§");
        let separated = replace_all(&escaped, ",", ";");
        let restored = replace_all(&separated, "§§§", ",");

        for id in ids.split(',') {
            let index = if let Some(&index) = self.class_index.get(id) {
                index
            } else {
                self.class_index.insert(id.to_owned(), self.classes.len());
                self.classes.push(FlowClassDef {
                    id: id.to_owned(),
                    styles: Vec::new(),
                    text_styles: Vec::new(),
                });
                self.classes.len() - 1
            };
            let Some(class) = self.classes.get_mut(index) else {
                continue;
            };
            for s in restored.split(';') {
                if s.contains("color") {
                    class.text_styles.push(s.replacen("fill", "bgFill", 1));
                }
                class.styles.push(s.to_owned());
            }
        }
    }

    /// Sets the diagram direction, mapping arrow symbols to names.
    pub(crate) fn set_direction(&mut self, dir: &str) {
        let mut d = js_trim(dir);
        if d.contains('<') {
            d = "RL";
        }
        if d.contains('^') {
            d = "BT";
        }
        if d.contains('>') {
            d = "LR";
        }
        if d.contains('v') {
            d = "TB";
        }
        if d == "TD" {
            d = "TB";
        }
        self.direction = Some(d.to_owned());
    }

    /// Adds a class to the nodes, edges, and flows named in a comma-separated list.
    pub(crate) fn set_class(&mut self, ids: &str, class_name: &str) {
        for id in ids.split(',') {
            if let Some(node) = self.node_mut(id) {
                node.classes.push(class_name.to_owned());
            }
            if let Some(edge) = self.edge_by_id.get(id).and_then(|&i| self.edges.get_mut(i)) {
                edge.classes.push(class_name.to_owned());
            }
            if let Some(subgraph) = self
                .subgraph_lookup
                .get(id)
                .and_then(|&i| self.subgraphs.get_mut(i))
            {
                subgraph.classes.push(class_name.to_owned());
            }
        }
    }

    /// Sets the tooltip of each node in a comma-separated list.
    pub(crate) fn set_tooltip(&mut self, ids: &str, tooltip: &str) {
        for id in ids.split(',') {
            if let Some(&index) = self.tooltip_index.get(id) {
                if let Some(entry) = self.tooltips.get_mut(index) {
                    tooltip.clone_into(&mut entry.1);
                }
            } else {
                self.tooltip_index
                    .insert(id.to_owned(), self.tooltips.len());
                self.tooltips.push((id.to_owned(), tooltip.to_owned()));
            }
        }
    }

    /// Sets the click target of each node in a comma-separated list and marks them clickable.
    pub(crate) fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
        let url = js_trim(link);
        for id in ids.split(',') {
            if let Some(vertex) = self.node_mut(id) {
                vertex.link = (!url.is_empty()).then(|| url::safe_url(url));
                vertex.link_target = target.map(str::to_owned);
            }
        }
        self.set_class(ids, "clickable");
    }

    /// Marks nodes clickable. Callbacks are never executed, as in Mermaid's strict mode.
    pub(crate) fn set_click_event(&mut self, ids: &str) {
        self.set_class(ids, "clickable");
    }

    /// Sets the accessible title without leading whitespace.
    pub(crate) fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(
            text.trim_start_matches(|c| is_space(u32::from(c)))
                .to_owned(),
        );
    }

    /// Sets the accessible description, removing indentation after line breaks.
    pub(crate) fn set_acc_description(&mut self, text: &str) {
        // `text.replace(/\n\s+/g, '\n')`.
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars.next_if(|&next| is_space(u32::from(next))).is_some() {}
            }
        }
        self.acc_descr = Some(out);
    }

    /// Adds a flow from the items its body collected and returns its id.
    pub(crate) fn add_subgraph(
        &mut self,
        id_text: Option<&FlowText>,
        list: &[DocItem],
        title_text: Option<&FlowText>,
    ) -> String {
        let id = id_text.map(|id| js_trim(&id.text).to_owned());
        let title = title_text.map_or("", |title| title.text.as_str());

        let mut seen = BTreeSet::new();
        let mut node_list: Vec<String> = Vec::new();
        let mut dir = None;
        let mut add = |item: &String| {
            if !js_trim(item).is_empty() && seen.insert(item.clone()) {
                node_list.push(item.clone());
            }
        };
        for entry in list {
            match entry {
                DocItem::Str(item) => add(item),
                DocItem::Ids(items) => items.iter().for_each(&mut add),
                DocItem::Dir(value) => dir = Some((*value).to_owned()),
            }
        }
        if dir.is_none() && self.options.inherit_dir {
            dir = self
                .direction
                .clone()
                .or_else(|| self.options.direction.clone());
        }

        let id = id.unwrap_or_else(|| {
            let mut generated = String::from("subGraph");
            push_u32(
                &mut generated,
                u32::try_from(self.sub_count).unwrap_or(u32::MAX),
            );
            generated
        });
        self.sub_count += 1;

        // A flow never contains itself, a globally scoped node, or a node another flow already
        // holds.
        let nodes: Vec<String> = node_list
            .into_iter()
            .filter(|node| {
                *node != id && !self.global_nodes.contains(node) && !self.members.contains(node)
            })
            .collect();
        for node in &nodes {
            self.members.insert(node.clone());
        }

        let title = js_trim(title);
        if let Some(&index) = self.subgraph_lookup.get(&id) {
            // A repeated id adds its members to the first flow. Its title and direction win.
            if let Some(existing) = self.subgraphs.get_mut(index) {
                existing.nodes.extend(nodes);
                if !title.is_empty() {
                    title.clone_into(&mut existing.title);
                }
                if let Some(dir) = dir.filter(|dir| !dir.is_empty()) {
                    existing.dir = Some(dir);
                }
            }
            return id;
        }

        self.subgraph_lookup
            .insert(id.clone(), self.subgraphs.len());
        self.subgraphs.push(AgentSubgraph {
            classes: Vec::new(),
            dir,
            id: id.clone(),
            label_type: title_text.map_or(LabelType::Markdown, |title| title.r#type),
            metadata: None,
            nodes,
            title: title.to_owned(),
        });
        id
    }

    /// Keeps every node named inside a `global` block out of the flows that mention it.
    pub(crate) fn add_global(&mut self, list: &[DocItem]) {
        let mut release = false;
        let mut add = |item: &String| {
            let id = js_trim(item);
            if id.is_empty() {
                return;
            }
            self.global_nodes.insert(id.to_owned());
            if self.members.remove(id) {
                release = true;
            }
        };
        for entry in list {
            match entry {
                DocItem::Str(item) => add(item),
                DocItem::Ids(items) => items.iter().for_each(&mut add),
                DocItem::Dir(_) => {}
            }
        }
        if !release {
            return;
        }

        let global_nodes = &self.global_nodes;
        for subgraph in &mut self.subgraphs {
            subgraph.nodes.retain(|node| !global_nodes.contains(node));
        }
    }

    /// Adds a connector, or retitles one, and returns its id, or an empty id for none.
    pub(crate) fn add_connector(
        &mut self,
        text: Option<&FlowText>,
        title: Option<&FlowText>,
    ) -> String {
        let Some(id) = text
            .map(|text| js_trim(&text.text))
            .filter(|id| !id.is_empty())
        else {
            return String::new();
        };
        let title = title.map_or("", |title| js_trim(&title.text));

        if !self.connector_index.contains(id) {
            let created = AgentNode {
                classes: Vec::new(),
                dir: None,
                id: id.to_owned(),
                is_connector: true,
                label_type: LabelType::Text,
                link: None,
                link_target: None,
                metadata: None,
                props: None,
                styles: Vec::new(),
                text: Some(if title.is_empty() { id } else { title }.to_owned()),
                r#type: None,
            };
            self.connector_index.insert(id.to_owned());
            self.connectors.push(id.to_owned());
            // `Map.set` keeps an existing key's place.
            if let Some(&index) = self.node_index.get(id) {
                if let Some(node) = self.nodes.get_mut(index) {
                    *node = created;
                }
            } else {
                self.node_index.insert(id.to_owned(), self.nodes.len());
                self.nodes.push(created);
            }
        } else if !title.is_empty()
            && let Some(connector) = self.node_mut(id)
        {
            connector.text = Some(title.to_owned());
        }
        id.to_owned()
    }

    /// Pele's `destructLink`: reads an arrow, and its opening half when the label sits inside it.
    pub(crate) fn destruct_link(end: &str, start: Option<&str>) -> LinkInfo {
        let (r#type, stroke, length) = destruct_end_link(end);
        if let Some(start) = start.filter(|start| !start.is_empty()) {
            // The first half of a split arrow only says whether the line is dotted.
            let opening = if start.contains('.') {
                "dotted"
            } else {
                "normal"
            };
            if opening != stroke {
                return LinkInfo {
                    edge_semantic: None,
                    id: None,
                    length: None,
                    stroke: "INVALID",
                    text: None,
                    r#type: "INVALID",
                };
            }
        }
        LinkInfo {
            edge_semantic: edge_semantic(r#type, stroke),
            id: None,
            length: Some(length),
            stroke,
            text: None,
            r#type,
        }
    }

    /// What a node is: `tool`, `action`, `input`, `refdoc`, `decision`, or `task`.
    pub(crate) fn vertex_kind(vertex: &AgentNode) -> &'static str {
        if is_tool_definition(vertex) {
            return "tool";
        }
        match vertex.r#type.as_deref().map(resolve_shape_alias) {
            Some("hexagon" | "hex") => "action",
            Some("lean-right" | "lean_right") => "input",
            Some("lin-doc" | "lined-document") => "refdoc",
            Some("diamond") => "decision",
            _ => "task",
        }
    }

    fn position(&self, loc: Option<Loc>) -> ElementPosition {
        let start_line = loc.map_or(0, |loc| loc.first_line);
        let start_column = loc.map_or(0, |loc| loc.first_column);
        ElementPosition {
            end_column: loc.map_or(start_column, |loc| loc.last_column),
            end_line: loc.map_or(start_line, |loc| loc.last_line) + self.options.line_offset,
            start_column,
            start_line: start_line + self.options.line_offset,
        }
    }

    fn push_mapping(&mut self, id: &str, r#type: &'static str, position: ElementPosition) -> usize {
        let index = self.mappings.len();
        self.mappings.push(ElementMapping {
            id: id.to_owned(),
            position,
            r#type,
        });
        if !self.first_mapping.contains_key(id) {
            self.first_mapping.insert(id.to_owned(), index);
        }
        index
    }

    /// Records where a node statement sits. A bare reference becomes an attachment if a metadata
    /// block follows it.
    pub(crate) fn add_vertex_mapping(&mut self, id: &str, shaped: bool, loc: Loc) {
        if id.is_empty() {
            return;
        }
        let position = self.position(Some(loc));
        let index = self.push_mapping(id, "vertex", position);
        if !shaped {
            self.bare_mappings.insert(index);
        }
    }

    /// Widens a node's span over the metadata block that follows it.
    pub(crate) fn extend_vertex_mapping(&mut self, id: &str, loc: Loc) {
        if id.is_empty() {
            return;
        }
        let end = self.position(Some(loc));
        for index in (0..self.mappings.len()).rev() {
            let Some(mapping) = self.mappings.get_mut(index) else {
                continue;
            };
            if mapping.r#type != "vertex" || mapping.id != id {
                continue;
            }
            if self.bare_mappings.remove(&index) {
                mapping.r#type = "attachment";
            }
            let p = &mut mapping.position;
            if end.end_line > p.end_line
                || (end.end_line == p.end_line && end.end_column > p.end_column)
            {
                p.end_line = end.end_line;
                p.end_column = end.end_column;
            }
            return;
        }
        self.push_mapping(id, "attachment", end);
    }

    /// Records where an edge statement sits, named by the nodes it ends at.
    pub(crate) fn add_edge_mapping(&mut self, to_nodes: &[String], loc: Loc) {
        let ids: Vec<&str> = to_nodes
            .iter()
            .map(String::as_str)
            .filter(|id| !id.is_empty())
            .collect();
        let id = if ids.is_empty() {
            "edge".to_owned()
        } else {
            ids.join(">")
        };
        let position = self.position(Some(loc));
        self.push_mapping(&id, "edge", position);
    }

    fn block_mapping(&mut self, id: &str, r#type: &'static str, start: Loc, end: Loc) {
        if id.is_empty() {
            return;
        }
        let start = self.position(Some(start));
        let end = self.position(Some(end));
        self.push_mapping(
            id,
            r#type,
            ElementPosition {
                end_column: end.end_column,
                end_line: end.end_line,
                start_column: start.start_column,
                start_line: start.start_line,
            },
        );
    }

    /// Records where a flow block sits, from its keyword to its `end`.
    pub(crate) fn add_subgraph_mapping(&mut self, id: Option<&FlowText>, start: Loc, end: Loc) {
        let id = id.map_or("", |id| id.text.as_str());
        self.block_mapping(id, "subgraph", start, end);
    }

    /// Records where a connector statement sits, from its keyword to its separator.
    pub(crate) fn add_connector_mapping(&mut self, text: Option<&FlowText>, start: Loc, end: Loc) {
        let id = text.map_or("", |text| js_trim(&text.text));
        self.block_mapping(id, "connector", start, end);
    }

    /// Records a finding about node `node_id` once. The graph may be built more than once.
    pub(crate) fn emit(
        &mut self,
        id: &'static str,
        severity: &'static str,
        message: String,
        node_id: &str,
    ) {
        let diagnostic = Diagnostic {
            id,
            message,
            node_id: (!node_id.is_empty()).then(|| node_id.to_owned()),
            position: (!node_id.is_empty())
                .then(|| self.first_mapping.get(node_id))
                .flatten()
                .and_then(|&index| self.mappings.get(index))
                .map(|mapping| mapping.position),
            severity,
        };

        let same = self.reported.entry(node_id.to_owned()).or_default();
        for &index in same.iter() {
            if let Some(d) = self.diagnostics.get(index)
                && d.id == id
                && d.severity == severity
                && d.message == diagnostic.message
                && d.node_id == diagnostic.node_id
            {
                return;
            }
        }
        same.push(self.diagnostics.len());
        self.diagnostics.push(diagnostic);
    }
}

/// `str.replace(from, to)` with a global pattern, without `str::replace`.
fn replace_all(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        out.push_str(rest.get(..at).unwrap_or(""));
        out.push_str(to);
        rest = rest.get(at + from.len()..).unwrap_or("");
    }
    out.push_str(rest);
    out
}
