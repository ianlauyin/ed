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
    available(options).map_or(4000.0, |available| {
        80.0_f64.max(available - 2.0 * options.padding.unwrap_or(8.0))
    })
}

/// For a chart drawn to a set width: its `natural` one, or the host's when that is smaller, down
/// to `least`. `around` is what the chart adds outside that width.
pub(crate) fn cap_width(options: &RenderOptions, natural: f64, least: f64, around: f64) -> f64 {
    available(options).map_or(natural, |available| {
        least.min(natural).max(natural.min(available - around))
    })
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

    let breakpoint = options
        .direction_breakpoint
        .filter(|&b| b > 0.0)
        .unwrap_or(BREAKPOINT);
    if available >= breakpoint {
        return across;
    }
    let down = draw(true);
    if down.width() < across.width() {
        down
    } else {
        across
    }
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
    if tight.width() < loose.width() {
        tight
    } else {
        loose
    }
}

#[cfg(test)]
mod tests {
    //! Expectations come from Pele's fitting functions, given the same drawing functions.

    use super::{Drawn, cap_width, fit_width, runs_across, tighten, title_room, turn_to_fit};
    use crate::svg::builder::num;
    use crate::svg::root::RenderOptions;

    struct Drawing(f64);

    impl Drawn for Drawing {
        fn width(&self) -> f64 {
            self.0
        }
    }

    #[expect(clippy::type_complexity, reason = "a table of cases")]
    const CASES: &[(
        Option<f64>,
        Option<f64>,
        Option<bool>,
        Option<f64>,
        &str,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    )] = &[
        (
            None,
            None,
            None,
            None,
            "400:false=440",
            4000.0,
            400.0,
            400.0,
            480.0,
            480.0,
            360.0,
        ),
        (
            Some(0.0),
            None,
            None,
            None,
            "400:false=440",
            4000.0,
            400.0,
            400.0,
            480.0,
            480.0,
            360.0,
        ),
        (
            Some(f64::NAN),
            None,
            None,
            None,
            "400:false=440",
            4000.0,
            400.0,
            400.0,
            480.0,
            480.0,
            360.0,
        ),
        (
            Some(300.0),
            None,
            None,
            None,
            "400:false 400:true 260:true 246:true 244.6:true=300.14",
            284.0,
            300.0,
            270.0,
            260.0,
            480.0,
            280.0,
        ),
        (
            Some(150.0),
            Some(20.0),
            None,
            None,
            "400:false 400:true 120:true=188",
            110.0,
            150.0,
            120.0,
            260.0,
            480.0,
            280.0,
        ),
        (
            Some(50.0),
            None,
            None,
            None,
            "400:false 400:true 120:true=188",
            80.0,
            120.0,
            120.0,
            260.0,
            480.0,
            280.0,
        ),
        (
            Some(700.0),
            None,
            None,
            None,
            "400:false=440",
            684.0,
            400.0,
            400.0,
            480.0,
            480.0,
            360.0,
        ),
        (
            Some(500.0),
            None,
            Some(false),
            None,
            "400:false=440",
            484.0,
            400.0,
            400.0,
            480.0,
            480.0,
            360.0,
        ),
        (
            Some(500.0),
            None,
            None,
            Some(400.0),
            "400:false=440",
            484.0,
            400.0,
            400.0,
            480.0,
            480.0,
            360.0,
        ),
        (
            Some(500.0),
            None,
            None,
            Some(-1.0),
            "400:false=440",
            484.0,
            400.0,
            400.0,
            480.0,
            480.0,
            360.0,
        ),
    ];

    #[test]
    #[expect(clippy::float_cmp, reason = "exact values from Pele")]
    fn fitting_matches_pele() {
        for &(
            max_width,
            padding,
            auto_direction,
            direction_breakpoint,
            fit,
            room,
            cap,
            cap_around,
            turn,
            turn_wider,
            tight,
        ) in CASES
        {
            let options = RenderOptions {
                auto_direction,
                direction_breakpoint,
                max_width,
                padding,
                ..RenderOptions::default()
            };
            let mut calls = Vec::new();
            let drawn = fit_width(&options, 400.0, 120.0, |plot, narrow| {
                calls.push(format!("{}:{narrow}", num(plot)));
                Drawing(plot * 0.9 + 80.0)
            });
            assert_eq!(
                format!("{}={}", calls.join(" "), num(drawn.0)),
                fit,
                "{max_width:?}"
            );
            assert_eq!(title_room(&options), room, "{max_width:?}");
            assert_eq!(cap_width(&options, 400.0, 120.0, 0.0), cap, "{max_width:?}");
            assert_eq!(
                cap_width(&options, 400.0, 120.0, 30.0),
                cap_around,
                "{max_width:?}"
            );
            assert_eq!(
                turn_to_fit(&options, |down| Drawing(if down { 260.0 } else { 480.0 })).0,
                turn,
                "{max_width:?}"
            );
            assert_eq!(
                turn_to_fit(&options, |down| Drawing(if down { 520.0 } else { 480.0 })).0,
                turn_wider,
                "{max_width:?}"
            );
            assert_eq!(
                tighten(&options, |t| Drawing(if t { 280.0 } else { 360.0 })).0,
                tight,
                "{max_width:?}"
            );
        }
    }

    #[test]
    fn runs_across_matches_pele() {
        for &(direction, expected) in &[
            (None, false),
            (Some("LR"), true),
            (Some("RL"), true),
            (Some("TB"), false),
            (Some("BT"), false),
            (Some("TD"), false),
            (Some("lr"), false),
        ] {
            assert_eq!(runs_across(direction), expected);
        }
    }
}
