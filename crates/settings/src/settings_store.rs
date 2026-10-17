use anyhow::Result;
use collections::{HashMap, TypeIdHashMap, hash_map};
use gpui::{App, AsyncApp, BorrowAppContext, Global, SharedString, UpdateGlobal};

use std::{
    any::{Any, TypeId, type_name},
    fmt::Debug,
    str,
};
use util::rel_path::RelPath;

use crate::{
    SemanticTokenRules, WorktreeId,
    settings_content::{LanguageDefaultsContent, SettingsContent, merge_from::MergeFrom},
};

/// A typed view of the built-in settings.
pub trait Settings: 'static + Send + Sync + Sized {
    /// Read the value from the merged built-in settings.
    ///
    /// This function *should* panic if default values are missing,
    /// and the built-in constructor must supply all required values.
    fn from_settings(content: &SettingsContent) -> Self;

    #[track_caller]
    fn register(cx: &mut App)
    where
        Self: Sized,
    {
        SettingsStore::update_global(cx, |store, _| {
            store.register_setting::<Self>();
        });
    }

    #[track_caller]
    fn get<'a>(path: Option<SettingsLocation>, cx: &'a App) -> &'a Self
    where
        Self: Sized,
    {
        cx.global::<SettingsStore>().get(path)
    }

    #[track_caller]
    fn get_global(cx: &App) -> &Self
    where
        Self: Sized,
    {
        cx.global::<SettingsStore>().get(None)
    }

    #[track_caller]
    fn try_get(cx: &App) -> Option<&Self>
    where
        Self: Sized,
    {
        if cx.has_global::<SettingsStore>() {
            cx.global::<SettingsStore>().try_get(None)
        } else {
            None
        }
    }

    #[track_caller]
    fn try_read_global<R>(cx: &AsyncApp, f: impl FnOnce(&Self) -> R) -> Option<R>
    where
        Self: Sized,
    {
        cx.try_read_global(|s: &SettingsStore, _| f(s.get(None)))
    }

    #[track_caller]
    fn override_global(settings: Self, cx: &mut App)
    where
        Self: Sized,
    {
        cx.global_mut::<SettingsStore>().override_global(settings)
    }
}

pub struct RegisteredSetting {
    pub settings_value: fn() -> Box<dyn AnySettingValue>,
    pub from_settings: fn(&SettingsContent) -> Box<dyn Any>,
    pub id: fn() -> TypeId,
}

inventory::collect!(RegisteredSetting);

#[derive(Clone, Copy, Debug)]
pub struct SettingsLocation<'a> {
    pub worktree_id: WorktreeId,
    pub path: &'a RelPath,
}

/// One complete application-wide configuration, with language conventions filled
/// in from registered language metadata and no file-based settings layers.
pub struct SettingsStore {
    setting_values: TypeIdHashMap<Box<dyn AnySettingValue>>,
    settings: SettingsContent,
    language_defaults: Option<crate::AllLanguageSettingsContent>,
    language_semantic_token_rules: HashMap<SharedString, SemanticTokenRules>,
    merged_settings: SettingsContent,
}

impl Global for SettingsStore {}

#[derive(Default)]
pub struct DefaultSemanticTokenRules(pub SemanticTokenRules);

impl gpui::Global for DefaultSemanticTokenRules {}

#[doc(hidden)]
#[derive(Debug)]
pub struct SettingValue<T> {
    pub global_value: Option<T>,
}

#[doc(hidden)]
pub trait AnySettingValue: 'static + Send + Sync {
    fn setting_type_name(&self) -> &'static str;
    fn from_settings(&self, s: &SettingsContent) -> Box<dyn Any>;
    fn value_for_path(&self, path: Option<SettingsLocation>) -> &dyn Any;
    fn set_global_value(&mut self, value: Box<dyn Any>);
}

impl SettingsStore {
    pub fn new(cx: &mut App, settings: SettingsContent) -> Self {
        if !cx.has_global::<DefaultSemanticTokenRules>() {
            cx.set_global(DefaultSemanticTokenRules(
                crate::built_in_semantic_token_rules::rules(),
            ));
        }
        let merged_settings = settings.clone();
        let mut this = Self {
            setting_values: Default::default(),
            settings,
            language_defaults: None,
            language_semantic_token_rules: HashMap::default(),
            merged_settings,
        };
        this.load_settings_types();
        this
    }

    pub fn update<C, R>(cx: &mut C, f: impl FnOnce(&mut Self, &mut C) -> R) -> R
    where
        C: BorrowAppContext,
    {
        cx.update_global(f)
    }

    /// Add a new type of setting to the store.
    pub fn register_setting<T: Settings>(&mut self) {
        self.register_setting_internal(&RegisteredSetting {
            settings_value: || Box::new(SettingValue::<T> { global_value: None }),
            from_settings: |content| Box::new(T::from_settings(content)),
            id: || TypeId::of::<T>(),
        });
    }

    fn load_settings_types(&mut self) {
        for registered_setting in inventory::iter::<RegisteredSetting>() {
            self.register_setting_internal(registered_setting);
        }
    }

    fn register_setting_internal(&mut self, registered_setting: &RegisteredSetting) {
        let entry = self.setting_values.entry((registered_setting.id)());

        if matches!(entry, hash_map::Entry::Occupied(_)) {
            return;
        }

        let setting_value = entry.or_insert((registered_setting.settings_value)());
        let value = (registered_setting.from_settings)(&self.merged_settings);
        setting_value.set_global_value(value);
    }

    pub fn merged_settings(&self) -> &SettingsContent {
        &self.merged_settings
    }

    /// Get the value of a setting.
    ///
    /// Panics if the given setting type has not been registered, or if there is no
    /// value for this setting.
    pub fn get<T: Settings>(&self, path: Option<SettingsLocation>) -> &T {
        self.setting_values
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("unregistered setting type {}", type_name::<T>()))
            .value_for_path(path)
            .downcast_ref::<T>()
            .expect("no default value for setting type")
    }

    /// Get the value of a setting.
    ///
    /// Does not panic
    pub fn try_get<T: Settings>(&self, path: Option<SettingsLocation>) -> Option<&T> {
        self.setting_values
            .get(&TypeId::of::<T>())
            .map(|value| value.value_for_path(path))
            .and_then(|value| value.downcast_ref::<T>())
    }

    /// Override a typed setting until the next settings recomputation.
    pub fn override_global<T: Settings>(&mut self, value: T) {
        self.setting_values
            .get_mut(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("unregistered setting type {}", type_name::<T>()))
            .set_global_value(Box::new(value))
    }

    /// Replace application-wide preferences without reading or writing a file.
    pub fn set_settings(&mut self, settings: SettingsContent, cx: &mut App) {
        self.settings = settings;
        self.recompute_values();
        cx.refresh_windows();
    }

    pub fn settings(&self) -> &SettingsContent {
        &self.settings
    }

    /// Sets language-specific semantic token rules.
    ///
    /// These rules are registered by built-in language modules.
    /// They are stored separately from the global rules and are only
    /// applied to buffers of the matching language by the `SemanticTokenStylizer`.
    pub fn set_language_semantic_token_rules(
        &mut self,
        language: SharedString,
        rules: SemanticTokenRules,
    ) {
        self.language_semantic_token_rules.insert(language, rules);
    }

    /// Removes language-specific semantic token rules for the given language.
    pub fn remove_language_semantic_token_rules(&mut self, language: &str) {
        self.language_semantic_token_rules.remove(language);
    }

    /// Returns the language-specific semantic token rules for the given language,
    /// if any have been registered.
    pub fn language_semantic_token_rules(&self, language: &str) -> Option<&SemanticTokenRules> {
        self.language_semantic_token_rules.get(language)
    }

    pub fn set_language_defaults(
        &mut self,
        content: LanguageDefaultsContent,
        _cx: &mut App,
    ) -> Result<()> {
        self.language_defaults = Some(content.all_languages);
        self.recompute_values();
        Ok(())
    }

    fn recompute_values(&mut self) {
        self.merged_settings = self.settings.clone();
        if let Some(metadata) = &self.language_defaults {
            let languages = &mut self.merged_settings.project.all_languages.languages.0;
            for (name, defaults) in &metadata.languages.0 {
                let mut language = defaults.clone();
                if let Some(configured) = languages.get(name) {
                    language.merge_from(configured);
                }
                languages.insert(name.clone(), language);
            }
        }
        for setting_value in self.setting_values.values_mut() {
            let value = setting_value.from_settings(&self.merged_settings);
            setting_value.set_global_value(value);
        }
    }
}

impl Debug for SettingsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsStore")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl<T: Settings> AnySettingValue for SettingValue<T> {
    fn from_settings(&self, s: &SettingsContent) -> Box<dyn Any> {
        Box::new(T::from_settings(s))
    }

    fn setting_type_name(&self) -> &'static str {
        type_name::<T>()
    }

    fn value_for_path(&self, _path: Option<SettingsLocation>) -> &dyn Any {
        self.global_value
            .as_ref()
            .unwrap_or_else(|| panic!("no default value for setting {}", self.setting_type_name()))
    }

    fn set_global_value(&mut self, value: Box<dyn Any>) {
        self.global_value = Some(*value.downcast().unwrap());
    }
}
