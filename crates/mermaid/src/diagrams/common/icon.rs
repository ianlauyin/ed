//! A square for a named icon. Port of Pele's `src/diagrams/common/icon.ts`.

use crate::svg::builder::{esc_text, push_num};
use crate::text::icons::{IconResolver, icon_markup};

/// A square for icon `name` at (`x`, `y`), `side` wide, in the form labels use. The host's
/// resolver supplies the drawing, if any.
pub(crate) fn icon_svg(
    name: &str,
    x: f64,
    y: f64,
    side: f64,
    icons: Option<IconResolver<'_>>,
) -> String {
    let mut out = String::from("<svg class=\"ondine-icon\" data-icon=\"");
    out.push_str(&esc_text(name));
    out.push_str("\" x=\"");
    push_num(&mut out, x);
    out.push_str("\" y=\"");
    push_num(&mut out, y);
    out.push_str("\" width=\"");
    push_num(&mut out, side);
    out.push_str("\" height=\"");
    push_num(&mut out, side);
    out.push_str(
        "\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
    );
    out.push_str(&icon_markup(icons, name));
    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `iconSvg` with `bun` and a host that has one icon, never from this port.
    //! Pele's `pele-icon` class reads as `ondine-icon`.

    use super::*;

    #[test]
    fn icon_svg_matches_pele() {
        let icons = |name: &str| (name == "fa:fa-car").then(|| "<path d=\"M1 1\"/>".to_owned());
        let resolver: &dyn Fn(&str) -> Option<String> = &icons;
        let cases: &[(&str, f64, f64, f64, &str)] = &[
            (
                "fa:fa-car",
                1.0,
                2.345,
                16.0,
                "<svg class=\"ondine-icon\" data-icon=\"fa:fa-car\" x=\"1\" y=\"2.35\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M1 1\"/></svg>",
            ),
            (
                "a\"<&b",
                -0.004,
                1000.0,
                0.0,
                "<svg class=\"ondine-icon\" data-icon=\"a&quot;&lt;&amp;b\" x=\"0\" y=\"1000\" width=\"0\" height=\"0\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"></svg>",
            ),
            (
                "fa:fa-none",
                10.005,
                -3.0,
                12.5,
                "<svg class=\"ondine-icon\" data-icon=\"fa:fa-none\" x=\"10.01\" y=\"-3\" width=\"12.5\" height=\"12.5\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"></svg>",
            ),
        ];
        for &(name, x, y, side, expected) in cases {
            assert_eq!(
                icon_svg(name, x, y, side, Some(resolver)),
                expected,
                "{name:?}"
            );
        }
    }
}
