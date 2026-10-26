use gpui::FontFallbacks;
use gpui::FontFeatures;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFLocale;
use objc2_core_foundation::CFMutableArray;
use objc2_core_foundation::CFNumber;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CFType;
use objc2_core_text::CTFont;
use objc2_core_text::CTFontDescriptor;
use objc2_core_text::CTFontSymbolicTraits;
use objc2_core_text::kCTFontCascadeListAttribute;
use objc2_core_text::kCTFontFamilyNameAttribute;
use objc2_core_text::kCTFontFeatureSettingsAttribute;
use objc2_core_text::kCTFontOpenTypeFeatureTag;
use objc2_core_text::kCTFontOpenTypeFeatureValue;
use objc2_core_text::kCTFontSlantTrait;
use objc2_core_text::kCTFontTraitsAttribute;
use objc2_core_text::kCTFontURLAttribute;
use objc2_core_text::kCTFontWeightTrait;

use crate::font;

pub fn apply_features_and_fallbacks(
    font: &mut CFRetained<CTFont>,
    features: &FontFeatures,
    fallbacks: Option<&FontFallbacks>,
) -> anyhow::Result<()> {
    let feature_array = generate_feature_array(features);
    let mut keys: Vec<&CFString> = vec![unsafe { kCTFontFeatureSettingsAttribute }];
    let mut values: Vec<&CFType> = vec![feature_array.as_ref()];
    let fallback_array;
    if let Some(fallbacks) = fallbacks
        && !fallbacks.fallback_list().is_empty()
    {
        fallback_array = generate_fallback_array(fallbacks, font);
        keys.push(unsafe { kCTFontCascadeListAttribute });
        values.push(fallback_array.as_ref());
    }
    let attributes: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(&keys, &values);
    unsafe {
        let descriptor = CTFontDescriptor::with_attributes(attributes.as_opaque());
        *font = font.copy_with_attributes(0.0, std::ptr::null(), Some(&descriptor));
    }
    Ok(())
}

fn generate_feature_array(features: &FontFeatures) -> CFRetained<CFMutableArray<CFDictionary<CFString, CFType>>> {
    let feature_array: CFRetained<CFMutableArray<CFDictionary<CFString, CFType>>> = CFMutableArray::empty();
    for (tag, value) in features.tag_value_list() {
        let tag = CFString::from_str(tag);
        let value = CFNumber::new_i32((*value).cast_signed());
        let feature: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(
            &[unsafe { kCTFontOpenTypeFeatureTag }, unsafe { kCTFontOpenTypeFeatureValue }],
            &[tag.as_ref(), value.as_ref()],
        );
        feature_array.append(&*feature);
    }
    feature_array
}

fn generate_fallback_array(fallbacks: &FontFallbacks, font: &CTFont) -> CFRetained<CFMutableArray<CTFontDescriptor>> {
    let is_italic = unsafe { font.symbolic_traits() }.contains(CTFontSymbolicTraits::TraitItalic);
    let weight = font::trait_value(&font::traits(font), unsafe { kCTFontWeightTrait }).unwrap_or(0.0);
    let weight_value = CFNumber::new_f64(weight);
    let slant_value = CFNumber::new_f64(if is_italic { 1.0 } else { 0.0 });
    let traits: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(
        &[unsafe { kCTFontWeightTrait }, unsafe { kCTFontSlantTrait }],
        &[weight_value.as_ref(), slant_value.as_ref()],
    );

    let fallback_array: CFRetained<CFMutableArray<CTFontDescriptor>> = CFMutableArray::empty();
    for user_fallback in fallbacks.fallback_list() {
        let name = CFString::from_str(user_fallback);
        let attributes: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(
            &[unsafe { kCTFontFamilyNameAttribute }, unsafe { kCTFontTraitsAttribute }],
            &[name.as_ref(), traits.as_ref()],
        );
        let descriptor = unsafe { CTFontDescriptor::with_attributes(attributes.as_opaque()) };
        fallback_array.append(&*descriptor);
    }

    append_system_fallbacks(&fallback_array, font);
    fallback_array
}

fn append_system_fallbacks(fallback_array: &CFMutableArray<CTFontDescriptor>, font: &CTFont) {
    let preferred_languages = CFLocale::preferred_languages();
    let Some(default_fallbacks) = (unsafe { font.default_cascade_list_for_languages(preferred_languages.as_deref()) })
    else {
        return;
    };
    // SAFETY: Core Text documents the cascade list as holding font descriptors.
    let default_fallbacks: CFRetained<CFArray<CTFontDescriptor>> =
        unsafe { CFRetained::cast_unchecked(default_fallbacks) };
    for descriptor in default_fallbacks.iter() {
        if unsafe { descriptor.attribute(kCTFontURLAttribute) }.is_some() {
            fallback_array.append(&*descriptor);
        }
    }
}
