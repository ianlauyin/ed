//! Font queries on `CTFont`, in font units, plus CSS-style face matching.

use std::ptr::NonNull;

use anyhow::anyhow;
use gpui::Bounds;
use gpui::DevicePixels;
use gpui::FontMetrics;
use gpui::FontStyle;
use gpui::FontWeight;
use gpui::Result;
use gpui::Size;
use gpui::point;
use gpui::size;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFNumber;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CFType;
use objc2_core_foundation::CGRect;
use objc2_core_foundation::CGSize;
use objc2_core_graphics::CGFont;
use objc2_core_graphics::CGGlyph;
use objc2_core_text::CTFont;
use objc2_core_text::CTFontCollection;
use objc2_core_text::CTFontDescriptor;
use objc2_core_text::CTFontOrientation;
use objc2_core_text::CTFontSymbolicTraits;
use objc2_core_text::kCTFontFamilyNameAttribute;
use objc2_core_text::kCTFontSlantTrait;
use objc2_core_text::kCTFontWeightTrait;
use objc2_core_text::kCTFontWidthTrait;

/// A `CTFont` shared across threads; Core Text fonts are immutable and thread-safe.
#[derive(Clone)]
pub struct SyncFont(pub CFRetained<CTFont>);

// SAFETY: Apple documents `CTFont` as safe to use from multiple threads.
unsafe impl Send for SyncFont {}
unsafe impl Sync for SyncFont {}

impl std::ops::Deref for SyncFont {
    type Target = CTFont;

    fn deref(&self) -> &CTFont {
        &self.0
    }
}

/// Point size used for fonts loaded by family; metrics are scaled to font units anyway.
const LOAD_POINT_SIZE: f64 = 16.0;

/// Core Text normalized weights for CSS weights 100 through 900.
const WEIGHT_MAPPING: [f32; 9] = [-0.7, -0.5, -0.23, 0.0, 0.2, 0.3, 0.4, 0.6, 0.8];

/// CSS stretch for Core Text widths -1.0 through 1.0 in 0.25 steps.
const STRETCH_MAPPING: [f32; 9] = [0.5, 0.625, 0.75, 0.875, 1.0, 1.125, 1.25, 1.5, 2.0];
const STRETCH_NORMAL: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontProperties {
    pub style: FontStyle,
    pub weight: f32,
    pub stretch: f32,
}

pub fn family_fonts(family_name: &str) -> Result<Vec<CFRetained<CTFont>>> {
    let family_name = CFString::from_str(family_name);
    let attributes: CFRetained<CFDictionary<CFString, CFType>> =
        CFDictionary::from_slices(&[unsafe { kCTFontFamilyNameAttribute }], &[family_name.as_ref()]);
    let descriptor = unsafe { CTFontDescriptor::with_attributes(attributes.as_opaque()) };
    let descriptors = CFArray::from_objects(&[&*descriptor]);
    let collection = unsafe { CTFontCollection::with_font_descriptors(Some(descriptors.as_opaque()), None) };
    let fonts: Vec<CFRetained<CTFont>> = matching_descriptors(&collection)
        .map(|descriptors| {
            descriptors
                .iter()
                .map(|descriptor| unsafe {
                    CTFont::with_font_descriptor(&descriptor, LOAD_POINT_SIZE, std::ptr::null())
                })
                .collect()
        })
        .unwrap_or_default();
    if fonts.is_empty() { Err(anyhow!("no font family named {family_name:?}")) } else { Ok(fonts) }
}

pub fn matching_descriptors(collection: &CTFontCollection) -> Option<CFRetained<CFArray<CTFontDescriptor>>> {
    let descriptors = unsafe { collection.matching_font_descriptors() }?;
    // SAFETY: Core Text documents the array as holding font descriptors.
    Some(unsafe { CFRetained::cast_unchecked(descriptors) })
}

pub fn from_core_graphics_font(font: &CGFont) -> CFRetained<CTFont> {
    unsafe { CTFont::with_graphics_font(font, LOAD_POINT_SIZE, std::ptr::null(), None) }
}

fn units_per_point(font: &CTFont) -> f64 {
    f64::from(unsafe { font.units_per_em() }) / unsafe { font.size() }
}

/// A rect as `[min_x, min_y, max_x, max_y]`, scaled from points to font units.
fn rect_in_units(rect: CGRect, units_per_point: f32) -> [f32; 4] {
    let (min_x, min_y) = (rect.origin.x as f32, rect.origin.y as f32);
    let (max_x, max_y) = (min_x + rect.size.width as f32, min_y + rect.size.height as f32);
    [min_x * units_per_point, min_y * units_per_point, max_x * units_per_point, max_y * units_per_point]
}

fn bounds_from_rect([min_x, min_y, max_x, max_y]: [f32; 4]) -> Bounds<f32> {
    Bounds { origin: point(min_x, min_y), size: size(max_x - min_x, max_y - min_y) }
}

fn glyph_rect_in_units(font: &CTFont, glyph_id: u32) -> [f32; 4] {
    let mut glyph = glyph_id as CGGlyph;
    let rect = unsafe {
        font.bounding_rects_for_glyphs(CTFontOrientation::Default, NonNull::from(&mut glyph), std::ptr::null_mut(), 1)
    };
    rect_in_units(rect, units_per_point(font) as f32)
}

pub fn metrics(font: &CTFont) -> FontMetrics {
    let units_per_point = units_per_point(font);
    unsafe {
        FontMetrics {
            units_per_em: font.units_per_em(),
            ascent: (font.ascent() * units_per_point) as f32,
            descent: (-font.descent() * units_per_point) as f32,
            line_gap: (font.leading() * units_per_point) as f32,
            underline_position: (font.underline_position() * units_per_point) as f32,
            underline_thickness: (font.underline_thickness() * units_per_point) as f32,
            cap_height: (font.cap_height() * units_per_point) as f32,
            x_height: (font.x_height() * units_per_point) as f32,
            bounding_box: bounds_from_rect(rect_in_units(font.bounding_box(), units_per_point as f32)),
        }
    }
}

pub fn typographic_bounds(font: &CTFont, glyph_id: u32) -> Bounds<f32> {
    bounds_from_rect(glyph_rect_in_units(font, glyph_id))
}

pub fn advance(font: &CTFont, glyph_id: u32) -> Size<f32> {
    let mut glyph = glyph_id as CGGlyph;
    let mut advance = CGSize::new(0.0, 0.0);
    unsafe { font.advances_for_glyphs(CTFontOrientation::Default, NonNull::from(&mut glyph), &raw mut advance, 1) };
    let units_per_point = units_per_point(font) as f32;
    size(advance.width as f32 * units_per_point, advance.height as f32 * units_per_point)
}

pub fn glyph_for_char(font: &CTFont, ch: char) -> Option<u32> {
    let mut utf16 = [0; 2];
    let utf16 = ch.encode_utf16(&mut utf16);
    let mut glyphs: [CGGlyph; 2] = [0; 2];
    unsafe {
        font.glyphs_for_characters(
            NonNull::from(&mut utf16[0]),
            NonNull::from(&mut glyphs[0]),
            utf16.len().cast_signed(),
        )
    };
    (glyphs[0] != 0).then_some(u32::from(glyphs[0]))
}

/// Pixel bounds of a glyph rasterized at `point_size * scale_factor`, with the origin at the top left.
pub fn raster_bounds(font: &CTFont, glyph_id: u32, point_size: f32, scale_factor: f32) -> Bounds<DevicePixels> {
    let units_to_points = point_size / unsafe { font.units_per_em() } as f32;
    let [min_x, min_y, max_x, max_y] = glyph_rect_in_units(font, glyph_id).map(|value| value * units_to_points);
    let (width, height) = (max_x - min_x, max_y - min_y);
    let top = -min_y - height;
    let (right, bottom) = (min_x + width, top + height);
    let left = (min_x * scale_factor).floor() as i32;
    let top = (top * scale_factor).floor() as i32;
    let right = (right * scale_factor).ceil() as i32;
    let bottom = (bottom * scale_factor).ceil() as i32;
    Bounds {
        origin: point(DevicePixels(left), DevicePixels(top)),
        size: size(DevicePixels(right - left), DevicePixels(bottom - top)),
    }
}

/// `None` when the font lacks a weight, width, or slant trait, which some fonts do.
pub fn properties(font: &CTFont) -> Option<FontProperties> {
    let traits = traits(font);
    let slant = trait_value(&traits, unsafe { kCTFontSlantTrait })?;
    let weight = trait_value(&traits, unsafe { kCTFontWeightTrait })?;
    let width = trait_value(&traits, unsafe { kCTFontWidthTrait })?;
    let style = if unsafe { font.symbolic_traits() }.contains(CTFontSymbolicTraits::TraitItalic) {
        FontStyle::Italic
    } else if slant > 0.0 {
        FontStyle::Oblique
    } else {
        FontStyle::Normal
    };
    let weight = piecewise_linear_find_index(weight as f32, &WEIGHT_MAPPING) * 100.0 + 100.0;
    let stretch = piecewise_linear_lookup((width as f32 + 1.0) * 4.0, &STRETCH_MAPPING);
    Some(FontProperties { style, weight, stretch })
}

pub fn traits(font: &CTFont) -> CFRetained<CFDictionary<CFString, CFType>> {
    // SAFETY: Core Text documents the traits dictionary as keyed by strings.
    unsafe { CFRetained::cast_unchecked(font.traits()) }
}

pub fn trait_value(traits: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<f64> {
    traits.get(key)?.downcast::<CFNumber>().ok()?.as_f64()
}

fn piecewise_linear_lookup(index: f32, mapping: &[f32]) -> f32 {
    let lower = mapping[index.floor() as usize];
    let upper = mapping[index.ceil() as usize];
    lower + (upper - lower) * index.fract()
}

fn piecewise_linear_find_index(value: f32, mapping: &[f32]) -> f32 {
    let upper_index =
        match mapping.binary_search_by(|probe| probe.partial_cmp(&value).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(index) => return index as f32,
            Err(upper_index) => upper_index,
        };
    if upper_index == 0 || upper_index >= mapping.len() {
        return upper_index as f32;
    }
    let lower_index = upper_index - 1;
    let (lower, upper) = (mapping[lower_index], mapping[upper_index]);
    lower_index as f32 + (value - lower) / (upper - lower)
}

/// CSS Fonts level 3 face matching for a normal-stretch query.
pub fn find_best_match(candidates: &[FontProperties], style: FontStyle, weight: FontWeight) -> Option<usize> {
    let mut matching: Vec<usize> = (0..candidates.len()).collect();

    let matching_stretch = nearest(
        &matching,
        |ix| candidates[ix].stretch,
        STRETCH_NORMAL,
        |stretch| stretch < STRETCH_NORMAL,
        |stretch| stretch > STRETCH_NORMAL,
    )?;
    matching.retain(|&ix| candidates[ix].stretch == matching_stretch);

    let style_preference = match style {
        FontStyle::Italic => [FontStyle::Italic, FontStyle::Oblique, FontStyle::Normal],
        FontStyle::Oblique => [FontStyle::Oblique, FontStyle::Italic, FontStyle::Normal],
        FontStyle::Normal => [FontStyle::Normal, FontStyle::Oblique, FontStyle::Italic],
    };
    let matching_style =
        style_preference.into_iter().find(|style| matching.iter().any(|&ix| candidates[ix].style == *style))?;
    matching.retain(|&ix| candidates[ix].style == matching_style);

    let query = weight.0;
    let has_weight = |weight: f32| matching.iter().any(|&ix| candidates[ix].weight == weight);
    let matching_weight = if has_weight(query) {
        query
    } else if (400.0..450.0).contains(&query) && has_weight(500.0) {
        500.0
    } else if (450.0..=500.0).contains(&query) && has_weight(400.0) {
        400.0
    } else if query <= 500.0 {
        nearest(&matching, |ix| candidates[ix].weight, query, |weight| weight <= query, |_| true)?
    } else {
        nearest(&matching, |ix| candidates[ix].weight, query, |weight| weight >= query, |_| true)?
    };
    matching.retain(|&ix| candidates[ix].weight == matching_weight);

    matching.first().copied()
}

/// The exact value if present, else the closest value on the preferred side, else the closest overall.
fn nearest(
    indices: &[usize],
    value: impl Fn(usize) -> f32,
    query: f32,
    preferred: impl Fn(f32) -> bool,
    fallback: impl Fn(f32) -> bool,
) -> Option<f32> {
    let distance = |ix: &usize| (value(*ix) - query).abs();
    let by_distance = |a: &usize, b: &usize| distance(a).total_cmp(&distance(b));
    if indices.iter().any(|&ix| value(ix) == query) {
        return Some(query);
    }
    indices
        .iter()
        .filter(|&&ix| preferred(value(ix)))
        .min_by(|a, b| by_distance(a, b))
        .or_else(|| indices.iter().filter(|&&ix| fallback(value(ix))).min_by(|a, b| by_distance(a, b)))
        .map(|&ix| value(ix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(style: FontStyle, weight: f32) -> FontProperties {
        FontProperties { style, weight, stretch: STRETCH_NORMAL }
    }

    #[test]
    fn find_best_match_follows_css_rules() {
        let faces = [
            face(FontStyle::Normal, 300.0),
            face(FontStyle::Normal, 500.0),
            face(FontStyle::Normal, 700.0),
            face(FontStyle::Italic, 400.0),
        ];
        assert_eq!(find_best_match(&faces, FontStyle::Normal, FontWeight(400.0)), Some(1));
        assert_eq!(find_best_match(&faces, FontStyle::Normal, FontWeight(450.0)), Some(0));
        assert_eq!(find_best_match(&faces, FontStyle::Normal, FontWeight(600.0)), Some(2));
        assert_eq!(find_best_match(&faces, FontStyle::Normal, FontWeight(900.0)), Some(2));
        assert_eq!(find_best_match(&faces, FontStyle::Normal, FontWeight(100.0)), Some(0));
        assert_eq!(find_best_match(&faces, FontStyle::Oblique, FontWeight(700.0)), Some(3));
        assert_eq!(find_best_match(&[], FontStyle::Normal, FontWeight(400.0)), None);
    }

    #[test]
    fn weight_mapping_round_trips() {
        for (ix, &core_text_weight) in WEIGHT_MAPPING.iter().enumerate() {
            assert_eq!(
                piecewise_linear_find_index(core_text_weight, &WEIGHT_MAPPING) * 100.0 + 100.0,
                (ix + 1) as f32 * 100.0
            );
        }
        assert_eq!(piecewise_linear_lookup((0.0 + 1.0) * 4.0, &STRETCH_MAPPING), STRETCH_NORMAL);
    }
}
