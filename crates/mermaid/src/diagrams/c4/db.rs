//! The C4 diagram database. Port of Pele's `src/diagrams/c4/db.ts`.

use std::collections::BTreeMap;

use super::types::{C4Attr, C4Element, C4Named, C4Rel};
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// JavaScript's `parseInt` with no radix: optional whitespace and sign, then decimal digits, or
/// hex digits after `0x`. `NaN` when there are no digits.
pub(crate) fn parse_int(text: &str) -> f64 {
    let text = trim_start(text);
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..).unwrap_or("")),
        Some(b'+') => (false, text.get(1..).unwrap_or("")),
        _ => (false, text),
    };
    let hex = text
        .get(..2)
        .is_some_and(|head| head.eq_ignore_ascii_case("0x"));
    let value = if hex {
        let digits: &str = {
            let body = text.get(2..).unwrap_or("");
            let end = body
                .bytes()
                .position(|c| !c.is_ascii_hexdigit())
                .unwrap_or(body.len());
            body.get(..end).unwrap_or("")
        };
        if digits.is_empty() {
            return f64::NAN;
        }
        // ponytail: past 32 hex digits the value is summed in floating point, which may round
        // differently from V8 in the last place.
        digits.chars().fold(0.0, |value: f64, c| {
            value * 16.0 + f64::from(c.to_digit(16).unwrap_or(0))
        })
    } else {
        let end = text
            .bytes()
            .position(|c| !c.is_ascii_digit())
            .unwrap_or(text.len());
        match text.get(..end).filter(|digits| !digits.is_empty()) {
            Some(digits) => digits.parse().unwrap_or(f64::NAN),
            None => return f64::NAN,
        }
    };
    if negative { -value } else { value }
}

/// `text.replace(/\n\s+/g, '\n')`: whitespace after each line break removed, line breaks within
/// it included.
fn acc_description(text: &str) -> String {
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

/// Sets the named entry of `extra`, keeping the place of one already there.
fn set_extra(extra: &mut Option<Vec<(String, String)>>, key: &str, value: &str) {
    let entries = extra.get_or_insert_with(Vec::new);
    match entries.iter_mut().find(|(own, _)| own == key) {
        Some(entry) => value.clone_into(&mut entry.1),
        None => entries.push((key.to_owned(), value.to_owned())),
    }
}

/// A field of a shape or boundary that a statement fills by position.
#[derive(Clone, Copy)]
enum ElementField {
    BgColor,
    BorderColor,
    Descr,
    FontColor,
    LegendSprite,
    LegendText,
    Link,
    Shadowing,
    Shape,
    Sprite,
    Tags,
    Techn,
    Type,
}

impl C4Element {
    fn field(&mut self, field: ElementField) -> &mut Option<String> {
        match field {
            ElementField::BgColor => &mut self.bg_color,
            ElementField::BorderColor => &mut self.border_color,
            ElementField::Descr => &mut self.descr,
            ElementField::FontColor => &mut self.font_color,
            ElementField::LegendSprite => &mut self.legend_sprite,
            ElementField::LegendText => &mut self.legend_text,
            ElementField::Link => &mut self.link,
            ElementField::Shadowing => &mut self.shadowing,
            ElementField::Shape => &mut self.shape,
            ElementField::Sprite => &mut self.sprite,
            ElementField::Tags => &mut self.tags,
            ElementField::Techn => &mut self.techn,
            ElementField::Type => &mut self.r#type,
        }
    }

    /// Mermaid assigns a named argument to whatever property it names, structural ones
    /// included. Here it reaches only the known fields. Any other name is kept aside in `extra`.
    fn named(&mut self, attr: &C4Named) {
        let field = match attr.key.as_str() {
            "label" => {
                attr.value.clone_into(&mut self.label);
                return;
            }
            "type" => ElementField::Type,
            "descr" => ElementField::Descr,
            "techn" => ElementField::Techn,
            "sprite" => ElementField::Sprite,
            "tags" => ElementField::Tags,
            "link" => ElementField::Link,
            "bgColor" => ElementField::BgColor,
            "fontColor" => ElementField::FontColor,
            "borderColor" => ElementField::BorderColor,
            "shadowing" => ElementField::Shadowing,
            "shape" => ElementField::Shape,
            "legendText" => ElementField::LegendText,
            "legendSprite" => ElementField::LegendSprite,
            _ => {
                set_extra(&mut self.extra, &attr.key, &attr.value);
                return;
            }
        };
        *self.field(field) = Some(attr.value.clone());
    }

    /// A positional argument fills the field of its slot. A named one fills the field it names,
    /// whichever slot it is in.
    fn put(&mut self, field: ElementField, attr: Option<&C4Attr>) {
        match attr {
            None => {}
            Some(C4Attr::Text(text)) => *self.field(field) = Some(text.clone()),
            Some(C4Attr::Named(named)) => self.named(named),
        }
    }

    fn text(&mut self, field: ElementField, attr: Option<&C4Attr>) {
        match attr {
            None => {
                self.field(field).get_or_insert_with(String::new);
            }
            Some(_) => self.put(field, attr),
        }
    }

    fn set_label(&mut self, attr: Option<&C4Attr>) {
        self.label_attr = None;
        match attr {
            Some(C4Attr::Named(named)) => {
                self.label.clear();
                self.label_attr = Some(named.clone());
                self.named(named);
            }
            Some(C4Attr::Text(text)) => text.clone_into(&mut self.label),
            None => self.label.clear(),
        }
    }
}

/// A field of a relation that a statement fills by position.
#[derive(Clone, Copy)]
enum RelField {
    Descr,
    LineColor,
    Link,
    Sprite,
    Tags,
    Techn,
    TextColor,
}

impl C4Rel {
    fn field(&mut self, field: RelField) -> &mut Option<String> {
        match field {
            RelField::Descr => &mut self.descr,
            RelField::LineColor => &mut self.line_color,
            RelField::Link => &mut self.link,
            RelField::Sprite => &mut self.sprite,
            RelField::Tags => &mut self.tags,
            RelField::Techn => &mut self.techn,
            RelField::TextColor => &mut self.text_color,
        }
    }

    /// As [`C4Element::named`], with the offsets read as numbers.
    fn named(&mut self, attr: &C4Named) {
        let field = match attr.key.as_str() {
            "offsetX" => {
                self.offset_x = Some(parse_int(&attr.value));
                return;
            }
            "offsetY" => {
                self.offset_y = Some(parse_int(&attr.value));
                return;
            }
            "label" => {
                attr.value.clone_into(&mut self.label);
                return;
            }
            "descr" => RelField::Descr,
            "techn" => RelField::Techn,
            "sprite" => RelField::Sprite,
            "tags" => RelField::Tags,
            "link" => RelField::Link,
            "textColor" => RelField::TextColor,
            "lineColor" => RelField::LineColor,
            _ => {
                set_extra(&mut self.extra, &attr.key, &attr.value);
                return;
            }
        };
        *self.field(field) = Some(attr.value.clone());
    }

    fn put(&mut self, field: RelField, attr: Option<&C4Attr>) {
        match attr {
            None => {}
            Some(C4Attr::Text(text)) => *self.field(field) = Some(text.clone()),
            Some(C4Attr::Named(named)) => self.named(named),
        }
    }

    fn text(&mut self, field: RelField, attr: Option<&C4Attr>) {
        match attr {
            None => {
                self.field(field).get_or_insert_with(String::new);
            }
            Some(_) => self.put(field, attr),
        }
    }

    fn set_label(&mut self, attr: Option<&C4Attr>) {
        self.label_attr = None;
        match attr {
            Some(C4Attr::Named(named)) => {
                self.label.clear();
                self.label_attr = Some(named.clone());
                self.named(named);
            }
            Some(C4Attr::Text(text)) => text.clone_into(&mut self.label),
            None => self.label.clear(),
        }
    }
}

/// A shape or a boundary, by its index in its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementRef {
    /// A boundary.
    Boundary(usize),
    /// A shape.
    Shape(usize),
}

/// The parsed C4 diagram: Pele's `C4Db`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct C4Db {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title. Mermaid's grammar never sets it.
    pub(crate) acc_title: Option<String>,
    /// Boundaries. The first is the unnamed one that holds everything at the top level.
    pub(crate) boundaries: Vec<C4Element>,
    /// The top-level boundary is not listed, so a boundary a diagram names `global` is its own
    /// box. In Mermaid it replaces the top level and nothing is drawn.
    boundary_index: BTreeMap<String, usize>,
    /// Boundaries per row.
    pub(crate) boundary_in_row: f64,
    /// The header keyword: `C4Context`, `C4Container`, `C4Component`, `C4Dynamic` or
    /// `C4Deployment`.
    pub(crate) c4_type: Option<String>,
    current: usize,
    rel_index: BTreeMap<(String, String), usize>,
    /// Relations, in order.
    pub(crate) rels: Vec<C4Rel>,
    shape_index: BTreeMap<String, usize>,
    /// Shapes per row.
    pub(crate) shape_in_row: f64,
    /// Shapes, in order.
    pub(crate) shapes: Vec<C4Element>,
    stack: Vec<usize>,
    /// The title.
    pub(crate) title: Option<String>,
}

impl Default for C4Db {
    fn default() -> Self {
        Self {
            acc_descr: None,
            acc_title: None,
            boundaries: vec![C4Element {
                alias: "global".to_owned(),
                label: "global".to_owned(),
                r#type: Some("global".to_owned()),
                ..C4Element::default()
            }],
            boundary_index: BTreeMap::new(),
            boundary_in_row: 2.0,
            c4_type: None,
            current: 0,
            rel_index: BTreeMap::new(),
            rels: Vec::new(),
            shape_index: BTreeMap::new(),
            shape_in_row: 4.0,
            shapes: Vec::new(),
            stack: Vec::new(),
            title: None,
        }
    }
}

/// Finds or adds the element `alias` names, and sets its label from `title`.
fn element(
    list: &mut Vec<C4Element>,
    index: &mut BTreeMap<String, usize>,
    alias: Option<&C4Attr>,
    title: Option<&C4Attr>,
) -> usize {
    let found = match alias {
        Some(C4Attr::Text(alias)) => index.get(alias).copied(),
        _ => None,
    };
    let at = found.unwrap_or_else(|| {
        let at = list.len();
        let mut el = C4Element::default();
        match alias {
            Some(C4Attr::Text(alias)) => {
                el.alias.clone_from(alias);
                index.insert(alias.clone(), at);
            }
            Some(C4Attr::Named(named)) => {
                el.alias_attr = Some(named.clone());
                el.named(named);
            }
            None => {}
        }
        list.push(el);
        at
    });
    if let Some(el) = list.get_mut(at) {
        el.set_label(title);
    }
    at
}

impl C4Db {
    /// Sets the header keyword.
    pub(crate) fn set_c4_type(&mut self, r#type: &str) {
        self.c4_type = Some(r#type.to_owned());
    }

    /// Sets the title.
    pub(crate) fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_owned());
    }

    /// Sets the accessible description, without the indentation of its lines.
    pub(crate) fn set_acc_description(&mut self, descr: &str) {
        self.acc_descr = Some(acc_description(descr));
    }

    /// Adds a person or a system: alias, label, ?descr, ?sprite, ?tags, $link.
    pub(crate) fn add_person_or_system(&mut self, kind: &str, attrs: &[C4Attr]) {
        let at = element(
            &mut self.shapes,
            &mut self.shape_index,
            attrs.first(),
            attrs.get(1),
        );
        let current = self.current;
        if let Some(shape) = self.shapes.get_mut(at) {
            shape.text(ElementField::Descr, attrs.get(2));
            finish_shape(shape, kind, attrs, 3, current);
        }
    }

    /// Adds a container or a component: alias, label, ?techn, ?descr, ?sprite, ?tags, $link.
    pub(crate) fn add_container(&mut self, kind: &str, attrs: &[C4Attr]) {
        let at = element(
            &mut self.shapes,
            &mut self.shape_index,
            attrs.first(),
            attrs.get(1),
        );
        let current = self.current;
        if let Some(shape) = self.shapes.get_mut(at) {
            shape.text(ElementField::Techn, attrs.get(2));
            shape.text(ElementField::Descr, attrs.get(3));
            finish_shape(shape, kind, attrs, 4, current);
        }
    }

    fn boundary(&mut self, attrs: &[C4Attr], fallback: &str) -> usize {
        let at = element(
            &mut self.boundaries,
            &mut self.boundary_index,
            attrs.first(),
            attrs.get(1),
        );
        if let Some(boundary) = self.boundaries.get_mut(at) {
            match attrs.get(2) {
                None => boundary.r#type = Some(fallback.to_owned()),
                attr => boundary.put(ElementField::Type, attr),
            }
        }
        at
    }

    fn open(&mut self, boundary: usize) {
        if let Some(el) = self.boundaries.get_mut(boundary) {
            el.parent = Some(self.current);
        }
        self.stack.push(self.current);
        self.current = boundary;
    }

    /// Opens a person or system boundary: alias, label, ?type, ?tags, $link.
    pub(crate) fn add_person_or_system_boundary(&mut self, attrs: &[C4Attr], fallback: &str) {
        let at = self.boundary(attrs, fallback);
        if let Some(boundary) = self.boundaries.get_mut(at) {
            boundary.put(ElementField::Tags, attrs.get(3));
            boundary.put(ElementField::Link, attrs.get(4));
        }
        self.open(at);
    }

    /// Opens a container boundary.
    pub(crate) fn add_container_boundary(&mut self, attrs: &[C4Attr]) {
        self.add_person_or_system_boundary(attrs, "container");
    }

    /// Opens a deployment node: alias, label, ?type, ?descr, ?sprite, ?tags, $link.
    pub(crate) fn add_deployment_node(&mut self, node_type: &str, attrs: &[C4Attr]) {
        let at = self.boundary(attrs, "node");
        if let Some(boundary) = self.boundaries.get_mut(at) {
            boundary.text(ElementField::Descr, attrs.get(3));
            // Mermaid drops whatever is in the sprite slot, a named argument included.
            boundary.put(ElementField::Sprite, attrs.get(4));
            boundary.put(ElementField::Tags, attrs.get(5));
            boundary.put(ElementField::Link, attrs.get(6));
            boundary.node_type = Some(node_type.to_owned());
        }
        self.open(at);
    }

    /// Closes the innermost open boundary.
    pub(crate) fn pop_boundary_parse_stack(&mut self) {
        self.current = self.stack.pop().unwrap_or(0);
    }

    /// Adds or updates a relation: from, to, label, ?techn, ?descr, ?sprite, ?tags, $link.
    pub(crate) fn add_rel(&mut self, r#type: &str, attrs: &[C4Attr]) {
        let (Some(from), Some(to), Some(_)) = (attrs.first(), attrs.get(1), attrs.get(2)) else {
            return;
        };
        let key = match (from, to) {
            (C4Attr::Text(from), C4Attr::Text(to)) => Some((from.clone(), to.clone())),
            _ => None,
        };
        let found = key
            .as_ref()
            .and_then(|key| self.rel_index.get(key).copied());
        let at = found.unwrap_or_else(|| {
            let at = self.rels.len();
            let mut rel = C4Rel {
                r#type: r#type.to_owned(),
                ..C4Rel::default()
            };
            match from {
                C4Attr::Text(from) => rel.from.clone_from(from),
                C4Attr::Named(named) => rel.from_attr = Some(named.clone()),
            }
            match to {
                C4Attr::Text(to) => rel.to.clone_from(to),
                C4Attr::Named(named) => rel.to_attr = Some(named.clone()),
            }
            self.rels.push(rel);
            if let Some(key) = key {
                self.rel_index.insert(key, at);
            }
            at
        });
        if let Some(rel) = self.rels.get_mut(at) {
            r#type.clone_into(&mut rel.r#type);
            rel.set_label(attrs.get(2));
            rel.text(RelField::Techn, attrs.get(3));
            rel.text(RelField::Descr, attrs.get(4));
            rel.put(RelField::Sprite, attrs.get(5));
            rel.put(RelField::Tags, attrs.get(6));
            rel.put(RelField::Link, attrs.get(7));
        }
    }

    /// The shape or boundary named `alias`, shapes first.
    pub(crate) fn get_c4_shape(&self, alias: &str) -> Option<ElementRef> {
        self.shape_index
            .get(alias)
            .map(|&at| ElementRef::Shape(at))
            .or_else(|| {
                self.boundary_index
                    .get(alias)
                    .map(|&at| ElementRef::Boundary(at))
            })
    }

    fn element_mut(&mut self, at: ElementRef) -> Option<&mut C4Element> {
        match at {
            ElementRef::Shape(at) => self.shapes.get_mut(at),
            ElementRef::Boundary(at) => self.boundaries.get_mut(at),
        }
    }

    /// Restyles a shape or boundary: elementName, ?bgColor, ?fontColor, ?borderColor,
    /// ?shadowing, ?shape, ?sprite, ?techn, ?legendText, ?legendSprite.
    pub(crate) fn update_el_style(&mut self, attrs: &[C4Attr]) {
        let Some(C4Attr::Text(alias)) = attrs.first() else {
            return;
        };
        let Some(at) = self.get_c4_shape(alias) else {
            return;
        };
        let Some(el) = self.element_mut(at) else {
            return;
        };
        for (field, slot) in [
            (ElementField::BgColor, 1),
            (ElementField::FontColor, 2),
            (ElementField::BorderColor, 3),
            (ElementField::Shadowing, 4),
            (ElementField::Shape, 5),
            (ElementField::Sprite, 6),
            (ElementField::Techn, 7),
            (ElementField::LegendText, 8),
            (ElementField::LegendSprite, 9),
        ] {
            el.put(field, attrs.get(slot));
        }
    }

    /// Restyles a relation: from, to, ?textColor, ?lineColor, ?offsetX, ?offsetY.
    pub(crate) fn update_rel_style(&mut self, attrs: &[C4Attr]) {
        let (Some(C4Attr::Text(from)), Some(C4Attr::Text(to))) = (attrs.first(), attrs.get(1))
        else {
            return;
        };
        let Some(&at) = self.rel_index.get(&(from.clone(), to.clone())) else {
            return;
        };
        let Some(rel) = self.rels.get_mut(at) else {
            return;
        };
        rel.put(RelField::TextColor, attrs.get(2));
        rel.put(RelField::LineColor, attrs.get(3));
        match attrs.get(4) {
            Some(C4Attr::Text(x)) => rel.offset_x = Some(parse_int(x)),
            Some(C4Attr::Named(named)) => rel.named(named),
            None => {}
        }
        match attrs.get(5) {
            Some(C4Attr::Text(y)) => rel.offset_y = Some(parse_int(y)),
            Some(C4Attr::Named(named)) => rel.named(named),
            None => {}
        }
    }

    /// Sets how many shapes and boundaries go in a row: ?c4ShapeInRow, ?c4BoundaryInRow. As in
    /// Mermaid, the slot decides which one a value sets, not its name.
    pub(crate) fn update_layout_config(&mut self, attrs: &[C4Attr]) {
        let count = |attr: Option<&C4Attr>| match attr {
            None => f64::NAN,
            Some(C4Attr::Text(text)) => parse_int(text),
            Some(C4Attr::Named(named)) => parse_int(&named.value),
        };
        let shapes = count(attrs.first());
        let boundaries = count(attrs.get(1));
        if shapes >= 1.0 {
            self.shape_in_row = shapes;
        }
        if boundaries >= 1.0 {
            self.boundary_in_row = boundaries;
        }
    }
}

fn finish_shape(shape: &mut C4Element, kind: &str, attrs: &[C4Attr], at: usize, current: usize) {
    shape.put(ElementField::Sprite, attrs.get(at));
    shape.put(ElementField::Tags, attrs.get(at + 1));
    shape.put(ElementField::Link, attrs.get(at + 2));
    kind.clone_into(&mut shape.kind);
    shape.parent = Some(current);
}
