//! Fits a diagram to the width the host has, by narrowing a plot, turning its direction, or
//! tightening its spacing. Port of Pele's `src/diagrams/common/fit-width.ts`.
//!
//! The host options are [`crate::svg::root::RenderOptions`].

use crate::svg::root::RenderOptions;

/// A drawn diagram, as far as fitting it is concerned.
pub(crate) trait Drawn {
    /// The drawn width.
    fn width(&self) -> f64;
}

/// The width the host has, when it set a positive one.
fn available(options: &RenderOptions) -> Option<f64> {
    options.max_width.filter(|&width| width > 0.0)
}

/// Draws a chart whose plot can be any width so the whole diagram fits the width the host has.
/// What surrounds the plot (labels, a legend) is only known once it is drawn, so the plot gives up
/// the overflow and is drawn again, down to `least`. `draw` takes the plot width and whether the
/// chart did not fit at its `natural` width, for a chart that then arranges itself differently.
pub(crate) fn fit_width<R: Drawn>(
    options: &RenderOptions,
    natural: f64,
    least: f64,
    mut draw: impl FnMut(f64, bool) -> R,
) -> R {
    let result = draw(natural, false);
    let Some(available) = available(options).filter(|&available| result.width() > available) else {
        return result;
    };

    let floor = least.min(natural);
    let mut plot = natural;
    let mut result = draw(plot, true);
    let mut round = 0;
    while round < 3 && result.width() > available && plot > floor {
        plot = floor.max(plot - (result.width() - available));
        result = draw(plot, true);
        round += 1;
    }
    result
}

/// The width a title may take before it wraps.
pub(crate) fn title_room(options: &RenderOptions) -> f64 {
    available(options).map_or(4000.0, |available| 80.0_f64.max(available - 2.0 * options.padding.unwrap_or(8.0)))
}

/// For a chart drawn to a set width: its `natural` one, or the host's when that is smaller, down
/// to `least`. `around` is what the chart adds outside that width.
pub(crate) fn cap_width(options: &RenderOptions, natural: f64, least: f64, around: f64) -> f64 {
    available(options).map_or(natural, |available| least.min(natural).max(natural.min(available - around)))
}

/// The width under which a diagram may turn to run down.
const BREAKPOINT: f64 = 640.0;

/// For a diagram that can run across or down: draws it across, and down instead when the host
/// allows it, the width it has is under the breakpoint, across does not fit, and down is narrower.
pub(crate) fn turn_to_fit<R: Drawn>(options: &RenderOptions, mut draw: impl FnMut(bool) -> R) -> R {
    let across = draw(false);
    let Some(available) = available(options) else {
        return across;
    };
    if options.auto_direction == Some(false) || across.width() <= available {
        return across;
    }

    let breakpoint = options.direction_breakpoint.filter(|&b| b > 0.0).unwrap_or(BREAKPOINT);
    if available >= breakpoint {
        return across;
    }
    let down = draw(true);
    if down.width() < across.width() { down } else { across }
}

/// Whether a direction runs across.
pub(crate) fn runs_across(direction: Option<&str>) -> bool {
    matches!(direction, Some("LR" | "RL"))
}

/// Draws a diagram with room to spare, and tightly instead when that does not fit the width the
/// host has and tight is narrower.
pub(crate) fn tighten<R: Drawn>(options: &RenderOptions, mut draw: impl FnMut(bool) -> R) -> R {
    let loose = draw(false);
    if !available(options).is_some_and(|available| loose.width() > available) {
        return loose;
    }

    let tight = draw(true);
    if tight.width() < loose.width() { tight } else { loose }
}
