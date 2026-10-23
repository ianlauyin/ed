//! Turns the syntax tree into the model the renderer draws, in Mermaid's order. Port of Pele's
//! `src/diagrams/wardley/model.ts`.

use super::builder::{
    Flow, NodeInput, WardleyAnnotation, WardleyAxes, WardleyBuildResult, WardleyBuilder,
    WardleyForce, WardleyLink, WardleyNote, WardleyTrend,
};
use super::parser::{LabelAst, WardleyAst};
use crate::error::{Code, Error};
use crate::text::label::trim;
use crate::util::chars::is_space;
use crate::yaml::push_number;

/// The parsed map: Pele's `WardleyModel`.
#[derive(Clone, Debug)]
pub(crate) struct WardleyModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// What the builder collected.
    pub(crate) build: WardleyBuildResult,
    /// The title.
    pub(crate) title: Option<String>,
}

#[cold]
fn semantic(message: String) -> Error {
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("wardley");
    error
}

/// `value` in percent: values up to 1 are fractions, larger ones are already percentages.
fn to_percent(value: f64, context: &str) -> Result<f64, Error> {
    let normalized = if value <= 1.0 { value * 100.0 } else { value };
    #[expect(clippy::manual_range_contains, reason = "NaN passes, as in JavaScript")]
    if normalized < 0.0 || normalized > 100.0 {
        let mut message = String::from(context);
        message.push_str(" must be between 0-1 (decimal) or 0-100 (percentage). Received: ");
        push_number(&mut message, value);
        return Err(semantic(message));
    }
    Ok(normalized)
}

/// `context` followed by `suffix`.
fn join(context: &str, suffix: &str) -> String {
    let mut out = String::from(context);
    out.push_str(suffix);
    out
}

/// `(x, y)` in percent from a visibility and an evolution. Evolution is checked first.
fn to_coordinates(visibility: f64, evolution: f64, context: &str) -> Result<(f64, f64), Error> {
    let x = to_percent(evolution, &join(context, " evolution"))?;
    let y = to_percent(visibility, &join(context, " visibility"))?;
    Ok((x, y))
}

/// `{prefix}"{name}"`.
fn quoted(prefix: &str, name: &str) -> String {
    let mut out = String::from(prefix);
    out.push('"');
    out.push_str(name);
    out.push('"');
    out
}

fn flow_from_port(port: Option<&str>) -> Option<Flow> {
    match port {
        Some("+<>") => Some(Flow::Bidirectional),
        Some("+<") => Some(Flow::Backward),
        Some("+>") => Some(Flow::Forward),
        _ => None,
    }
}

/// The flow and quoted label of an arrow like `+'label'>`.
fn flow_from_arrow(arrow: Option<&str>) -> (Option<Flow>, Option<String>) {
    let Some(arrow) = arrow.filter(|arrow| arrow.starts_with('+')) else {
        return (None, None);
    };
    let label = (arrow.as_bytes().get(1) == Some(&b'\''))
        .then(|| {
            let end = arrow.get(2..)?.find('\'')? + 2;
            arrow.get(2..end).map(str::to_owned)
        })
        .flatten();
    let flow = if arrow.contains("<>") {
        Some(Flow::Bidirectional)
    } else if arrow.contains('<') {
        Some(Flow::Backward)
    } else if arrow.contains('>') {
        Some(Flow::Forward)
    } else {
        None
    };
    (flow, label)
}

fn offsets(label: Option<&LabelAst>) -> (Option<f64>, Option<f64>) {
    let Some(label) = label else {
        return (None, None);
    };
    let sign = |neg: bool| if neg { -1.0 } else { 1.0 };
    (
        Some(sign(label.neg_x) * label.offset_x),
        Some(sign(label.neg_y) * label.offset_y),
    )
}

/// Mermaid's `populateDb`: fills the builder from the syntax tree, in Mermaid's order.
///
/// # Errors
///
/// Returns a semantic error for a coordinate out of range or a pipeline without a component.
pub(crate) fn populate(ast: &WardleyAst, db: &mut WardleyBuilder) -> Result<(), Error> {
    if let Some((width, height)) = ast.size {
        db.set_size(width, height);
    }

    if let Some(stages) = &ast.evolution {
        db.set_axes(WardleyAxes {
            stage_boundaries: Some(stages.iter().filter_map(|stage| stage.boundary).collect()),
            stages: Some(
                stages
                    .iter()
                    .map(|stage| match &stage.second_name {
                        Some(second) => {
                            let mut name = trim(&stage.name).to_owned();
                            name.push_str(" / ");
                            name.push_str(trim(second));
                            name
                        }
                        None => trim(&stage.name).to_owned(),
                    })
                    .collect(),
            ),
        });
    }

    for anchor in &ast.anchors {
        let (x, y) = to_coordinates(
            anchor.visibility,
            anchor.evolution,
            &quoted("Anchor ", &anchor.name),
        )?;
        db.add_node(NodeInput {
            class_name: "anchor",
            id: anchor.name.clone(),
            inertia: None,
            label: anchor.name.clone(),
            label_offset_x: None,
            label_offset_y: None,
            source_strategy: None,
            x,
            y,
        });
    }

    for component in &ast.components {
        let (x, y) = to_coordinates(
            component.visibility,
            component.evolution,
            &quoted("Component ", &component.name),
        )?;
        let (label_offset_x, label_offset_y) = offsets(component.label.as_ref());
        db.add_node(NodeInput {
            class_name: "component",
            id: component.name.clone(),
            inertia: Some(Some(component.inertia)),
            label: component.name.clone(),
            label_offset_x,
            label_offset_y,
            source_strategy: Some(component.decorator.clone()),
            x,
            y,
        });
    }

    for note in &ast.notes {
        let (x, y) = to_coordinates(
            note.visibility,
            note.evolution,
            &quoted("Note ", &note.text),
        )?;
        db.add_note(WardleyNote {
            text: note.text.clone(),
            x,
            y,
        });
    }

    for pipeline in &ast.pipelines {
        let Some(y) = db.get_node(&pipeline.parent).map(|parent| parent.y) else {
            let mut message = quoted("Pipeline ", &pipeline.parent);
            message.push_str(" must reference an existing component with coordinates.");
            return Err(semantic(message));
        };
        db.start_pipeline(&pipeline.parent);
        for component in &pipeline.components {
            let mut id = pipeline.parent.clone();
            id.push('_');
            id.push_str(&component.name);
            let (label_offset_x, label_offset_y) = offsets(component.label.as_ref());
            let x = to_percent(
                component.evolution,
                &join(
                    &quoted("Pipeline component ", &component.name),
                    " evolution",
                ),
            )?;
            db.add_node(NodeInput {
                class_name: "pipeline-component",
                id: id.clone(),
                inertia: Some(None),
                label: component.name.clone(),
                label_offset_x,
                label_offset_y,
                source_strategy: Some(None),
                x,
                y,
            });
            db.add_pipeline_component(&pipeline.parent, &id);
        }
    }

    for link in &ast.links {
        let arrow = link.arrow.as_deref();
        let dashed = arrow.is_some_and(|arrow| arrow.contains("-.->") || arrow.contains(".-."));
        let (arrow_flow, arrow_label) = flow_from_arrow(arrow);
        let flow = flow_from_port(link.from_port.as_deref())
            .or_else(|| flow_from_port(link.to_port.as_deref()))
            .or(arrow_flow);
        let source = db.resolve_node_id(&link.from);
        let target = db.resolve_node_id(&link.to);
        db.add_link(WardleyLink {
            dashed,
            flow,
            label: arrow_label.or_else(|| link.link_label.clone()),
            source,
            target,
        });
    }

    for evolve in &ast.evolves {
        if let Some(y) = db.get_node(&evolve.component).map(|node| node.y) {
            let target_x = to_percent(
                evolve.target,
                &quoted("Evolve target for ", &evolve.component),
            )?;
            db.add_trend(WardleyTrend {
                node_id: evolve.component.clone(),
                target_x,
                target_y: y,
            });
        }
    }

    if let Some(&(x, y)) = ast.annotations.first() {
        let (x, y) = to_coordinates(x, y, "Annotations box")?;
        db.set_annotations_box(x, y);
    }

    for annotation in &ast.annotation {
        let mut context = String::from("Annotation ");
        push_number(&mut context, annotation.number);
        let (x, y) = to_coordinates(annotation.x, annotation.y, &context)?;
        db.add_annotation(WardleyAnnotation {
            coordinates: vec![(x, y)],
            number: annotation.number,
            text: Some(annotation.text.clone()),
        });
    }

    for accelerator in &ast.accelerators {
        let (x, y) = to_coordinates(
            accelerator.x,
            accelerator.y,
            &quoted("Accelerator ", &accelerator.name),
        )?;
        db.add_accelerator(WardleyForce {
            name: accelerator.name.clone(),
            x,
            y,
        });
    }

    for deaccelerator in &ast.deaccelerators {
        let (x, y) = to_coordinates(
            deaccelerator.x,
            deaccelerator.y,
            &quoted("Deaccelerator ", &deaccelerator.name),
        )?;
        db.add_deaccelerator(WardleyForce {
            name: deaccelerator.name.clone(),
            x,
            y,
        });
    }
    Ok(())
}

/// `text.replace(/\n\s+/g, '\n')`.
fn collapse_indents(text: &str) -> String {
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

/// Builds the model from the syntax tree, as Pele's `buildWardley` does. `title` comes from front
/// matter and gives way to the map's own.
///
/// # Errors
///
/// Returns the semantic errors of [`populate`].
pub(crate) fn build_wardley(ast: WardleyAst, title: Option<&str>) -> Result<WardleyModel, Error> {
    let mut db = WardleyBuilder::default();
    populate(&ast, &mut db)?;
    Ok(WardleyModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| collapse_indents(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| crate::text::label::trim_start(&text).to_owned()),
        build: db.build(),
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    })
}
