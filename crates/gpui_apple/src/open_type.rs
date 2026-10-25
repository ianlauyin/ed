use core_foundation::array::CFArray;
use core_foundation::array::CFArrayAppendValue;
use core_foundation::array::CFArrayCreateMutable;
use core_foundation::array::CFMutableArrayRef;
use core_foundation::array::kCFTypeArrayCallBacks;
use core_foundation::base::CFRelease;
use core_foundation::base::TCFType;
use core_foundation::base::kCFAllocatorDefault;
use core_foundation::dictionary::CFDictionaryCreate;
use core_foundation::dictionary::kCFTypeDictionaryKeyCallBacks;
use core_foundation::dictionary::kCFTypeDictionaryValueCallBacks;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::locale::CFLocaleCopyPreferredLanguages;
use core_text::font::CTFont;
use core_text::font::cascade_list_for_languages;
use core_text::font_descriptor::CTFontDescriptor;
use core_text::font_descriptor::CTFontDescriptorCreateWithAttributes;
use core_text::font_descriptor::TraitAccessors;
use core_text::font_descriptor::kCTFontCascadeListAttribute;
use core_text::font_descriptor::kCTFontFamilyNameAttribute;
use core_text::font_descriptor::kCTFontFeatureSettingsAttribute;
use core_text::font_descriptor::kCTFontItalicTrait;
use core_text::font_descriptor::kCTFontSlantTrait;
use core_text::font_descriptor::kCTFontTraitsAttribute;
use core_text::font_descriptor::kCTFontWeightTrait;
use gpui::FontFallbacks;
use gpui::FontFeatures;
use objc2_core_text::kCTFontOpenTypeFeatureTag;
use objc2_core_text::kCTFontOpenTypeFeatureValue;

pub fn apply_features_and_fallbacks(
    font: &mut CTFont,
    features: &FontFeatures,
    fallbacks: Option<&FontFallbacks>,
) -> anyhow::Result<()> {
    unsafe {
        let mut keys = vec![kCTFontFeatureSettingsAttribute];
        let mut values = vec![generate_feature_array(features)];
        if let Some(fallbacks) = fallbacks
            && !fallbacks.fallback_list().is_empty()
        {
            keys.push(kCTFontCascadeListAttribute);
            values.push(generate_fallback_array(fallbacks, font));
        }
        let attrs = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr() as _,
            values.as_ptr() as _,
            keys.len().cast_signed(),
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        );

        for value in &values {
            CFRelease(*value as _);
        }

        let new_descriptor = CTFontDescriptorCreateWithAttributes(attrs);
        CFRelease(attrs as _);
        let new_descriptor = CTFontDescriptor::wrap_under_create_rule(new_descriptor);
        // Borrow the same CF objects for the generated API, then transfer its
        // create-rule ownership back.
        let generated_font = &*font.as_concrete_TypeRef().cast::<objc2_core_text::CTFont>();
        let descriptor = &*new_descriptor.as_concrete_TypeRef().cast::<objc2_core_text::CTFontDescriptor>();
        let new_font = generated_font.copy_with_attributes(0.0, std::ptr::null(), Some(descriptor));
        *font = CTFont::wrap_under_create_rule(objc2_core_foundation::CFRetained::into_raw(new_font).as_ptr().cast());

        Ok(())
    }
}

fn generate_feature_array(features: &FontFeatures) -> CFMutableArrayRef {
    unsafe {
        let feature_array = CFArrayCreateMutable(kCFAllocatorDefault, 0, &raw const kCFTypeArrayCallBacks);
        for (tag, value) in features.tag_value_list() {
            let keys = [kCTFontOpenTypeFeatureTag as *const _, kCTFontOpenTypeFeatureValue as *const _];
            let tag = CFString::new(tag);
            let value = CFNumber::from((*value).cast_signed());
            let values = [tag.as_CFTypeRef(), value.as_CFTypeRef()];
            let dict = CFDictionaryCreate(
                kCFAllocatorDefault,
                &raw const keys as _,
                &raw const values as _,
                2,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            CFArrayAppendValue(feature_array, dict as _);
            CFRelease(dict as _);
        }
        feature_array
    }
}

fn generate_fallback_array(fallbacks: &FontFallbacks, font: &mut CTFont) -> CFMutableArrayRef {
    unsafe {
        let symbolic_traits = font.symbolic_traits();
        let all_traits = font.all_traits();

        let fallback_array = CFArrayCreateMutable(kCFAllocatorDefault, 0, &raw const kCFTypeArrayCallBacks);
        for user_fallback in fallbacks.fallback_list() {
            let name = CFString::from(user_fallback.as_str());

            let traits_keys = [kCTFontWeightTrait, kCTFontSlantTrait];
            let weight_value = CFNumber::from(all_traits.normalized_weight());
            let slant_value = CFNumber::from(if (symbolic_traits & kCTFontItalicTrait) != 0 { 1.0 } else { 0.0 });
            let traits_values = [weight_value.as_CFTypeRef(), slant_value.as_CFTypeRef()];
            let traits = CFDictionaryCreate(
                kCFAllocatorDefault,
                &raw const traits_keys as _,
                &raw const traits_values as _,
                traits_keys.len().cast_signed(),
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            drop(weight_value);
            drop(slant_value);

            let attr_keys = [kCTFontFamilyNameAttribute, kCTFontTraitsAttribute];
            let attr_values = [name.as_CFTypeRef(), traits as _];
            let attrs = CFDictionaryCreate(
                kCFAllocatorDefault,
                &raw const attr_keys as _,
                &raw const attr_values as _,
                attr_keys.len().cast_signed(),
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            CFRelease(traits as _);

            let fallback_desc = CTFontDescriptorCreateWithAttributes(attrs);
            CFRelease(attrs as _);

            CFArrayAppendValue(fallback_array, fallback_desc as _);
            CFRelease(fallback_desc as _);
        }

        append_system_fallbacks(fallback_array, font);
        fallback_array
    }
}

fn append_system_fallbacks(fallback_array: CFMutableArrayRef, font: &CTFont) {
    unsafe {
        let preferred_languages: CFArray<CFString> = CFArray::wrap_under_create_rule(CFLocaleCopyPreferredLanguages());

        let default_fallbacks = cascade_list_for_languages(font, &preferred_languages);

        for desc in default_fallbacks.iter().filter(|desc| desc.font_path().is_some()) {
            CFArrayAppendValue(fallback_array, desc.as_concrete_TypeRef() as _);
        }
    }
}
