//! A square for a named icon. Port of Pele's `src/diagrams/common/icon.ts`.

use crate::svg::builder::esc_text;
use crate::svg::builder::push_num;
use crate::text::icons::IconResolver;
use crate::text::icons::icon_markup;

/// A square for icon `name` at (`x`, `y`), `side` wide, in the form labels use. The host's
/// resolver supplies the drawing, if any.
pub(crate) fn icon_svg(name: &str, x: f64, y: f64, side: f64, icons: Option<IconResolver<'_>>) -> String {
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
