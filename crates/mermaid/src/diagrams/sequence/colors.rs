//! Whether the first word of a box title is a color. Port of Pele's
//! `src/diagrams/sequence/colors.ts`.
//!
//! Mermaid asks the browser. Pele has no browser, so it knows the color keywords and the `rgb()`
//! and `hsl()` notations. Other functions, such as `calc()` and `var()`, are not recognized.

use crate::text::label::trim;
use crate::util::chars::{is_digit, is_space};

const KEYWORDS: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "green",
    "greenyellow",
    "grey",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
    "transparent",
    "currentcolor",
    "inherit",
    "initial",
    "unset",
    "revert",
    "accentcolor",
    "accentcolortext",
    "activetext",
    "buttonborder",
    "buttonface",
    "buttontext",
    "canvas",
    "canvastext",
    "field",
    "fieldtext",
    "graytext",
    "highlight",
    "highlighttext",
    "linktext",
    "mark",
    "marktext",
    "selecteditem",
    "selecteditemtext",
    "visitedtext",
    "activeborder",
    "activecaption",
    "appworkspace",
    "background",
    "buttonhighlight",
    "buttonshadow",
    "captiontext",
    "inactiveborder",
    "inactivecaption",
    "inactivecaptiontext",
    "infobackground",
    "infotext",
    "menu",
    "menutext",
    "scrollbar",
    "threeddarkshadow",
    "threedface",
    "threedhighlight",
    "threedlightshadow",
    "threedshadow",
    "window",
    "windowframe",
    "windowtext",
];

/// What a term of a color function is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum K {
    Bad,
    Number,
    Percent,
    Angle,
    None,
}

/// Pele's `/^[+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:e[+-]?\d+)?(%|deg|grad|rad|turn)?$/i`.
fn kind(term: &str) -> K {
    if term.eq_ignore_ascii_case("none") {
        return K::None;
    }
    let b = term.as_bytes();
    let digits = |mut i: usize| {
        let start = i;
        while b.get(i).is_some_and(|&c| is_digit(u32::from(c))) {
            i += 1;
        }
        (i > start).then_some(i)
    };
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    i = if let Some(end) = digits(i) {
        match b.get(end) {
            Some(b'.') => match digits(end + 1) {
                Some(after) => after,
                None => end,
            },
            _ => end,
        }
    } else if b.get(i) == Some(&b'.') {
        let Some(end) = digits(i + 1) else {
            return K::Bad;
        };
        end
    } else {
        return K::Bad;
    };
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if let Some(end) = digits(j) {
            i = end;
        }
    }
    let unit = b.get(i..).unwrap_or(&[]);
    if unit.is_empty() {
        return K::Number;
    }
    if unit == b"%" {
        return K::Percent;
    }
    if ["deg", "grad", "rad", "turn"]
        .iter()
        .any(|u| unit.eq_ignore_ascii_case(u.as_bytes()))
    {
        return K::Angle;
    }
    K::Bad
}

fn is_alpha(term: &str, modern: bool) -> bool {
    let k = kind(term);
    k == K::Number || k == K::Percent || (modern && k == K::None)
}

fn is_function(color: &str) -> bool {
    let hsl = color.starts_with("hsl");
    let Some(open) = color.find('(') else {
        return false;
    };
    let name = color.get(..open).unwrap_or("");
    if !matches!(name, "rgb" | "rgba" | "hsl" | "hsla") {
        return false;
    }
    if color.encode_utf16().count() > 120 || color.find(')') != Some(color.len() - 1) {
        return false;
    }
    let inner = trim(color.get(open + 1..color.len() - 1).unwrap_or(""));
    let legacy = inner.contains(',');
    let mut alpha = None;
    let parts: Vec<&str> = if legacy {
        let mut parts: Vec<&str> = inner.split(',').map(trim).collect();
        if parts.len() == 4 {
            alpha = parts.pop();
        }
        parts
    } else {
        let slash: Vec<&str> = inner.split('/').collect();
        if slash.len() > 2 {
            return false;
        }
        if let Some(&after) = slash.get(1) {
            alpha = Some(trim(after));
        }
        split_space(trim(slash.first().copied().unwrap_or("")))
    };
    if parts.len() != 3 || alpha.is_some_and(|alpha| !is_alpha(alpha, !legacy)) {
        return false;
    }
    let kinds: Vec<K> = parts.iter().map(|part| kind(part)).collect();
    let (Some(&k0), Some(&k1), Some(&k2)) = (kinds.first(), kinds.get(1), kinds.get(2)) else {
        return false;
    };
    if legacy {
        if hsl {
            return (k0 == K::Number || k0 == K::Angle) && k1 == K::Percent && k2 == K::Percent;
        }
        return (k0 == K::Number || k0 == K::Percent) && k1 == k0 && k2 == k0;
    }
    kinds.iter().enumerate().all(|(i, &k)| {
        k == K::Number
            || k == K::None
            || if hsl && i == 0 {
                k == K::Angle
            } else {
                k == K::Percent
            }
    })
}

/// JavaScript's `text.split(/\s+/)`.
fn split_space(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if is_space(u32::from(c)) {
            parts.push(text.get(start..at).unwrap_or(""));
            let mut end = at + c.len_utf8();
            while let Some(&(next, c)) = chars.peek() {
                if !is_space(u32::from(c)) {
                    break;
                }
                end = next + c.len_utf8();
                chars.next();
            }
            start = end;
        }
    }
    parts.push(text.get(start..).unwrap_or(""));
    parts
}

/// Whether a browser would take `color` as a CSS color.
pub(crate) fn is_css_color(color: &str) -> bool {
    if color.contains('(') {
        return is_function(color);
    }
    KEYWORDS
        .iter()
        .any(|keyword| keyword.eq_ignore_ascii_case(color))
}
