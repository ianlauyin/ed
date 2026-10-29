use std::borrow::Cow;
use std::char;
use std::convert::TryFrom;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;

use anyhow::anyhow;
use collections::HashMap;
use collections::HashSet;
use gpui::Bounds;
use gpui::DevicePixels;
use gpui::Font;
use gpui::FontFallbacks;
use gpui::FontFeatures;
use gpui::FontId;
use gpui::FontMetrics;
use gpui::FontRun;
use gpui::GlyphId;
use gpui::Hsla;
use gpui::LineLayout;
use gpui::Pixels;
use gpui::PlatformTextSystem;
use gpui::RenderGlyphParams;
use gpui::Result;
use gpui::Rgba;
use gpui::SUBPIXEL_VARIANTS_X;
use gpui::ShapedGlyph;
use gpui::ShapedRun;
use gpui::SharedString;
use gpui::Size;
use gpui::TextRenderingMode;
use gpui::point;
use gpui::px;
use gpui::swap_rgba_pa_to_bgra;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFData;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFMutableAttributedString;
use objc2_core_foundation::CFNumber;
use objc2_core_foundation::CFPreferencesCopyAppValue;
use objc2_core_foundation::CFRange;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CFType;
use objc2_core_foundation::CGFloat;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::kCFPreferencesCurrentApplication;
use objc2_core_graphics::CGBitmapContextCreate;
use objc2_core_graphics::CGColorSpace;
use objc2_core_graphics::CGContext;
use objc2_core_graphics::CGDataProvider;
use objc2_core_graphics::CGFont;
use objc2_core_graphics::CGGlyph;
use objc2_core_graphics::CGImageAlphaInfo;
use objc2_core_graphics::CGTextDrawingMode;
use objc2_core_text::CTFont;
use objc2_core_text::CTFontCollection;
use objc2_core_text::CTFontDescriptor;
use objc2_core_text::CTLine;
use objc2_core_text::CTRun;
use objc2_core_text::kCTFontAttributeName;
use objc2_core_text::kCTFontFamilyNameAttribute;
use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;
use smallvec::SmallVec;

use crate::font;
use crate::font::SyncFont;
use crate::open_type::apply_features_and_fallbacks;

/// Apple text system using CoreText for font shaping.
pub struct AppleTextSystem(RwLock<AppleTextSystemState>);

#[derive(Clone, PartialEq, Eq, Hash)]
struct FontKey {
    font_family: SharedString,
    font_features: FontFeatures,
    font_fallbacks: Option<FontFallbacks>,
}

struct AppleTextSystemState {
    memory_fonts: Vec<SyncFont>,
    fonts: Vec<SyncFont>,
    font_selections: HashMap<Font, FontId>,
    font_ids_by_postscript_name: HashMap<String, FontId>,
    font_ids_by_font_key: HashMap<FontKey, SmallVec<[FontId; 4]>>,
    postscript_names_by_font_id: HashMap<FontId, String>,
}

impl AppleTextSystem {
    /// Create a new AppleTextSystem.
    pub fn new() -> Self {
        Self(RwLock::new(AppleTextSystemState {
            memory_fonts: Vec::new(),
            fonts: Vec::new(),
            font_selections: HashMap::default(),
            font_ids_by_postscript_name: HashMap::default(),
            font_ids_by_font_key: HashMap::default(),
            postscript_names_by_font_id: HashMap::default(),
        }))
    }
}

impl Default for AppleTextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformTextSystem for AppleTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.0.write().add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let collection = unsafe { CTFontCollection::from_available_fonts(None) };
        if let Some(descriptors) = font::matching_descriptors(&collection) {
            names.extend(descriptors.iter().filter_map(|descriptor| family_name(&descriptor)));
        }
        names.extend(self.0.read().memory_fonts.iter().map(|font| unsafe { font.family_name() }.to_string()));
        names
    }

    fn font_id(&self, font: &Font) -> Result<FontId> {
        let lock = self.0.upgradable_read();
        if let Some(font_id) = lock.font_selections.get(font) {
            Ok(*font_id)
        } else {
            let mut lock = RwLockUpgradableReadGuard::upgrade(lock);
            let font_key = FontKey {
                font_family: font.family.clone(),
                font_features: font.features.clone(),
                font_fallbacks: font.fallbacks.clone(),
            };
            let candidates = if let Some(font_ids) = lock.font_ids_by_font_key.get(&font_key) {
                font_ids.as_slice()
            } else {
                let font_ids = lock.load_family(&font.family, &font.features, font.fallbacks.as_ref())?;
                lock.font_ids_by_font_key.insert(font_key.clone(), font_ids);
                lock.font_ids_by_font_key[&font_key].as_ref()
            };

            // `load_family` only registers fonts that have properties.
            let candidate_properties = candidates
                .iter()
                .map(|font_id| font::properties(&lock.fonts[font_id.0]))
                .collect::<Option<SmallVec<[_; 4]>>>()
                .ok_or_else(|| anyhow!("font in family {:?} has no traits", font.family))?;

            let ix = font::find_best_match(&candidate_properties, font.style, font.weight)
                .ok_or_else(|| anyhow!("no font in family {:?}", font.family))?;

            let font_id = candidates[ix];
            lock.font_selections.insert(font.clone(), font_id);
            Ok(font_id)
        }
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        font::metrics(&self.0.read().fonts[font_id.0])
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        Ok(font::typographic_bounds(&self.0.read().fonts[font_id.0], glyph_id.0))
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.0.read().advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.0.read().glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.0.read().raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        glyph_id: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.0.read().rasterize_glyph(glyph_id, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        self.0.write().layout_line(text, font_size, font_runs)
    }

    fn recommended_rendering_mode(&self, _font_id: FontId, _font_size: Pixels) -> TextRenderingMode {
        TextRenderingMode::Grayscale
    }

    fn glyph_dilation_for_color(&self, color: Hsla) -> u8 {
        // When font smoothing is enabled, CoreGraphics thickens glyph strokes by an amount that
        // depends on the foreground color's luminance. We replicate the logic used by CoreGraphics
        // to select between the different levels of dilation.
        if !font_smoothing_allowed_by_user() {
            return 0;
        }
        let rgba: Rgba = color.into();
        let luminance = 0.2126 * rgba.r + 0.7152 * rgba.g + 0.0722 * rgba.b;
        let level = ((4.0 * luminance) + 0.5).floor() as i32;
        level.clamp(0, 4) as u8
    }
}

fn font_smoothing_allowed_by_user() -> bool {
    static ALLOWED: OnceLock<bool> = OnceLock::new();
    *ALLOWED.get_or_init(|| {
        let key = CFString::from_static_str("AppleFontSmoothing");
        let Some(value) = CFPreferencesCopyAppValue(&key, unsafe { kCFPreferencesCurrentApplication }) else {
            return true;
        };
        let Ok(number) = value.downcast::<CFNumber>() else {
            return true;
        };
        // Only an explicit value of `0` means that font smoothing is disabled.
        number.as_i64() != Some(0)
    })
}

impl AppleTextSystemState {
    fn add_fonts(&mut self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        for bytes in fonts {
            let data = match bytes {
                Cow::Borrowed(embedded_font) => CFData::from_static_bytes(embedded_font),
                Cow::Owned(bytes) => CFData::from_bytes(&bytes),
            };
            let font = CGDataProvider::with_cf_data(Some(&data))
                .and_then(|data_provider| CGFont::with_data_provider(&data_provider))
                .ok_or_else(|| anyhow!("Could not load a font."))?;
            self.memory_fonts.push(SyncFont(font::from_core_graphics_font(&font)));
        }
        Ok(())
    }

    fn load_family(
        &mut self,
        name: &str,
        features: &FontFeatures,
        fallbacks: Option<&FontFallbacks>,
    ) -> Result<SmallVec<[FontId; 4]>> {
        let name = gpui::font_name_with_fallbacks(name, ".AppleSystemUIFont");

        let mut font_ids = SmallVec::new();
        let mut postscript_names_seen = HashSet::default();
        let memory_fonts: Vec<CFRetained<CTFont>> = self
            .memory_fonts
            .iter()
            .filter(|font| unsafe { font.family_name() }.to_string() == name)
            .map(|font| font.0.clone())
            .collect();
        let family = if memory_fonts.is_empty() { font::family_fonts(name)? } else { memory_fonts };
        for mut font in family {
            apply_features_and_fallbacks(&mut font, features, fallbacks)?;
            // Text measurement in the editor relies on the 'm' glyph.
            let display_name = unsafe { font.display_name() }.to_string();
            let postscript_name = unsafe { font.post_script_name() }.to_string();
            if font::glyph_for_char(&font, 'm').is_none() {
                log::warn!("font '{display_name}' has no 'm' character and was not loaded");
                continue;
            }

            if font::properties(&font).is_none() {
                log::error!("Failed to read traits for font {display_name:?} (PostScript name {postscript_name:?})");
                continue;
            }

            // Dedup is scoped to this single `load_family` call (issue #55472).
            // The same family can be reloaded later under a different `FontKey`
            // (different features/fallbacks); a global check against
            // `font_ids_by_postscript_name` would skip every already-registered
            // font and leave the second call's `font_ids` empty.
            if !postscript_names_seen.insert(postscript_name.clone()) {
                log::warn!(
                    "skipping duplicate font {:?} with PostScript name {:?} \
                     in family {:?}",
                    display_name,
                    postscript_name,
                    name,
                );
                continue;
            }
            let font_id = FontId(self.fonts.len());
            font_ids.push(font_id);
            self.font_ids_by_postscript_name.insert(postscript_name.clone(), font_id);
            self.postscript_names_by_font_id.insert(font_id, postscript_name);
            self.fonts.push(SyncFont(font));
        }
        Ok(font_ids)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        Ok(font::advance(&self.fonts[font_id.0], glyph_id.0))
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        font::glyph_for_char(&self.fonts[font_id.0], ch).map(GlyphId)
    }

    fn id_for_native_font(&mut self, requested_font: &CTFont) -> FontId {
        let postscript_name = unsafe { requested_font.post_script_name() }.to_string();
        if let Some(font_id) = self.font_ids_by_postscript_name.get(&postscript_name) {
            *font_id
        } else {
            let font_id = FontId(self.fonts.len());
            self.font_ids_by_postscript_name.insert(postscript_name.clone(), font_id);
            self.postscript_names_by_font_id.insert(font_id, postscript_name);
            let graphics_font = unsafe { requested_font.graphics_font(std::ptr::null_mut()) };
            self.fonts.push(SyncFont(font::from_core_graphics_font(&graphics_font)));
            font_id
        }
    }

    fn is_emoji(&self, font_id: FontId) -> bool {
        self.postscript_names_by_font_id.get(&font_id).is_some_and(|postscript_name| {
            postscript_name == "AppleColorEmoji" || postscript_name == ".AppleColorEmojiUI"
        })
    }

    fn raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let bounds = font::raster_bounds(
            &self.fonts[params.font_id.0],
            params.glyph_id.0,
            params.font_size.into(),
            params.scale_factor,
        );

        // Expand the bounds by 1 pixel on each side to give CG room for anti-aliasing.
        Ok(bounds.dilate(DevicePixels(1)))
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        glyph_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        if glyph_bounds.size.width.0 == 0 || glyph_bounds.size.height.0 == 0 {
            anyhow::bail!("glyph bounds are empty");
        } else {
            // Add an extra pixel when the subpixel variant isn't zero to make room for anti-aliasing.
            let mut bitmap_size = glyph_bounds.size;
            if params.subpixel_variant.x > 0 {
                bitmap_size.width += DevicePixels(1);
            }
            if params.subpixel_variant.y > 0 {
                bitmap_size.height += DevicePixels(1);
            }
            let bitmap_size = bitmap_size;

            let mut bytes;

            let (bytes_per_pixel, color_space, alpha_info) = if params.is_emoji {
                (4, CGColorSpace::new_device_rgb(), CGImageAlphaInfo::PremultipliedLast)
            } else {
                (1, CGColorSpace::new_device_gray(), CGImageAlphaInfo::Only)
            };
            let width = bitmap_size.width.0 as usize;
            let height = bitmap_size.height.0 as usize;
            bytes = vec![0; width * bytes_per_pixel * height];
            let cx = unsafe {
                CGBitmapContextCreate(
                    bytes.as_mut_ptr().cast::<c_void>(),
                    width,
                    height,
                    8,
                    width * bytes_per_pixel,
                    color_space.as_deref(),
                    alpha_info.0,
                )
            }
            .ok_or_else(|| anyhow!("failed to create a glyph bitmap context"))?;

            // Move the origin to bottom left and account for scaling, this
            // makes drawing text consistent with `font::raster_bounds`.
            CGContext::translate_ctm(
                Some(&cx),
                -glyph_bounds.origin.x.0 as CGFloat,
                (glyph_bounds.origin.y.0 + glyph_bounds.size.height.0) as CGFloat,
            );
            CGContext::scale_ctm(Some(&cx), params.scale_factor as CGFloat, params.scale_factor as CGFloat);

            let subpixel_shift = params.subpixel_variant.map(|v| v as f32 / SUBPIXEL_VARIANTS_X as f32);
            CGContext::set_text_drawing_mode(Some(&cx), CGTextDrawingMode::Fill);
            CGContext::set_allows_antialiasing(Some(&cx), true);
            CGContext::set_should_antialias(Some(&cx), true);
            CGContext::set_allows_font_subpixel_positioning(Some(&cx), true);
            CGContext::set_should_subpixel_position_fonts(Some(&cx), true);
            CGContext::set_allows_font_subpixel_quantization(Some(&cx), false);
            CGContext::set_should_subpixel_quantize_fonts(Some(&cx), false);

            if params.dilation > 0 {
                let luminance = params.dilation as f64 * 0.25;
                CGContext::set_should_smooth_fonts(Some(&cx), true);
                CGContext::set_gray_fill_color(Some(&cx), luminance, 1.0);
            } else {
                CGContext::set_gray_fill_color(Some(&cx), 0.0, 1.0);
            }
            let mut glyph = params.glyph_id.0 as CGGlyph;
            let mut position = CGPoint::new(
                (subpixel_shift.x / params.scale_factor) as CGFloat,
                (subpixel_shift.y / params.scale_factor) as CGFloat,
            );
            unsafe {
                let font = self.fonts[params.font_id.0].copy_with_attributes(
                    f32::from(params.font_size) as CGFloat,
                    std::ptr::null(),
                    None,
                );
                font.draw_glyphs(NonNull::from(&mut glyph), NonNull::from(&mut position), 1, &cx);
            }

            if params.is_emoji {
                // Convert from RGBA with premultiplied alpha to BGRA with straight alpha.
                for pixel in bytes.as_chunks_mut::<4>().0 {
                    swap_rgba_pa_to_bgra(pixel);
                }
            }

            Ok((bitmap_size, bytes))
        }
    }

    fn layout_line(&mut self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        // Construct the attributed string, converting UTF8 ranges to UTF16 ranges.
        let string = CFMutableAttributedString::new(None, 0).expect("failed to create an attributed string");
        let mut max_ascent = 0.0f32;
        let mut max_descent = 0.0f32;

        {
            let mut text = text;
            // CoreText merges adjacent runs that end up in equal fonts, so every other run that
            // inserts text is set a hair larger to keep ligatures from forming across them. The first
            // run keeps its exact size: SF tracks by exact point size, so a nudged size draws wider
            // or narrower than AppKit sets the same text.
            let mut break_ligature = false;
            for run in font_runs {
                let text_run;
                (text_run, text) = text.split_at(run.len);

                let utf16_start = string.length(); // insert at end of string
                // note: replace_string may silently ignore codepoints it dislikes (e.g., BOM at start of string)
                unsafe {
                    CFMutableAttributedString::replace_string(
                        Some(&string),
                        CFRange::new(utf16_start, 0),
                        Some(&CFString::from_str(text_run)),
                    );
                }
                let utf16_end = string.length();

                let length = utf16_end - utf16_start;
                let cf_range = CFRange::new(utf16_start, length);
                let font = &self.fonts[run.font_id.0];

                let font_metrics = font::metrics(font);
                let font_scale = f32::from(font_size) / font_metrics.units_per_em as f32;
                max_ascent = max_ascent.max(font_metrics.ascent * font_scale);
                max_descent = max_descent.max(-font_metrics.descent * font_scale);

                let font_size = if break_ligature { px(f32::from(font_size).next_up()) } else { font_size };
                unsafe {
                    let font = font.copy_with_attributes(f32::from(font_size).into(), std::ptr::null(), None);
                    CFMutableAttributedString::set_attribute(
                        Some(&string),
                        cf_range,
                        Some(kCTFontAttributeName),
                        Some(&font),
                    );
                }
                if length > 0 {
                    break_ligature = !break_ligature;
                }
            }
        }
        // Retrieve the glyphs from the shaped line, converting UTF16 offsets to UTF8 offsets.
        let line = unsafe { CTLine::with_attributed_string(string.as_ref()) };
        // SAFETY: Core Text documents the glyph runs as an array of `CTRun`s.
        let glyph_runs: CFRetained<CFArray<CTRun>> = unsafe { CFRetained::cast_unchecked(line.glyph_runs()) };
        let mut runs = <Vec<ShapedRun>>::with_capacity(glyph_runs.len());
        let mut ix_converter = StringIndexConverter::new(text);
        for run in glyph_runs.iter() {
            // SAFETY: Run attributes are keyed by attribute name strings.
            let attributes: CFRetained<CFDictionary<CFString, CFType>> =
                unsafe { CFRetained::cast_unchecked(run.attributes()) };
            let font = attributes
                .get(unsafe { kCTFontAttributeName })
                .and_then(|font| font.downcast::<CTFont>().ok())
                .expect("Core Text runs always carry a font");
            let font_id = self.id_for_native_font(&font);

            let glyph_count = usize::try_from(unsafe { run.glyph_count() }).unwrap_or(0);
            let glyphs = match runs.last_mut() {
                Some(run) if run.font_id == font_id => &mut run.glyphs,
                _ => {
                    runs.push(ShapedRun { font_id, glyphs: Vec::with_capacity(glyph_count) });
                    &mut runs.last_mut().unwrap().glyphs
                }
            };
            let run_glyphs = unsafe { run_values(&run, glyph_count, CTRun::glyphs_ptr, CTRun::glyphs) };
            let run_positions = unsafe { run_values(&run, glyph_count, CTRun::positions_ptr, CTRun::positions) };
            let run_string_indices =
                unsafe { run_values(&run, glyph_count, CTRun::string_indices_ptr, CTRun::string_indices) };
            for ((&glyph_id, position), &glyph_utf16_ix) in
                run_glyphs.iter().zip(run_positions.iter()).zip(run_string_indices.iter())
            {
                let glyph_utf16_ix = usize::try_from(glyph_utf16_ix).unwrap();
                if ix_converter.utf16_ix > glyph_utf16_ix {
                    // We cannot reuse current index converter, as it can only seek forward. Restart the search.
                    ix_converter = StringIndexConverter::new(text);
                }
                ix_converter.advance_to_utf16_ix(glyph_utf16_ix);
                glyphs.push(ShapedGlyph {
                    id: GlyphId(glyph_id as u32),
                    position: point(position.x as f32, position.y as f32).map(px),
                    index: ix_converter.utf8_ix,
                    is_emoji: self.is_emoji(font_id),
                });
            }
        }
        let width =
            unsafe { line.typographic_bounds(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        LineLayout {
            runs,
            font_size,
            width: (width as f32).into(),
            ascent: max_ascent.into(),
            descent: max_descent.into(),
            len: text.len(),
        }
    }
}

#[derive(Debug, Clone)]
struct StringIndexConverter<'a> {
    text: &'a str,
    /// Index in UTF-8 bytes
    utf8_ix: usize,
    /// Index in UTF-16 code units
    utf16_ix: usize,
}

impl<'a> StringIndexConverter<'a> {
    const fn new(text: &'a str) -> Self {
        Self { text, utf8_ix: 0, utf16_ix: 0 }
    }

    fn advance_to_utf16_ix(&mut self, utf16_target: usize) {
        for (ix, c) in self.text[self.utf8_ix..].char_indices() {
            if self.utf16_ix >= utf16_target {
                self.utf8_ix += ix;
                return;
            }
            self.utf16_ix += c.len_utf16();
        }
        self.utf8_ix = self.text.len();
    }
}

/// A run's values, borrowed when Core Text exposes its internal buffer and copied otherwise.
///
/// # Safety
///
/// `count` must be the run's glyph count, and `ptr` and `copy` must be a matching pair of `CTRun`
/// accessors.
unsafe fn run_values<T: Copy + Default>(
    run: &CTRun,
    count: usize,
    ptr: unsafe fn(&CTRun) -> *const T,
    copy: unsafe fn(&CTRun, CFRange, NonNull<T>),
) -> Cow<'_, [T]> {
    let values = unsafe { ptr(run) };
    if !values.is_null() {
        // SAFETY: A non-null buffer holds all `count` values and lives as long as the run.
        return Cow::Borrowed(unsafe { std::slice::from_raw_parts(values, count) });
    }
    let mut values = vec![T::default(); count];
    if let Some(buffer) = NonNull::new(values.as_mut_ptr()) {
        // A zero-length range copies the whole run.
        unsafe { copy(run, CFRange::new(0, 0), buffer) };
    }
    Cow::Owned(values)
}

fn family_name(descriptor: &CTFontDescriptor) -> Option<String> {
    let value = unsafe { descriptor.attribute(kCTFontFamilyNameAttribute) }?;
    Some(value.downcast::<CFString>().ok()?.to_string())
}
