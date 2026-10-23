//! Host icons for labels. Port of Pele's `src/text/icons.ts`.
//!
//! Pele keeps the host's resolver in module state for the render in progress and memoizes its
//! answers. Ondine has no module state: every function that lays out or draws a label takes the
//! resolver as an explicit `Option<IconResolver>` argument, and `None` means the host has no
//! icons. A host that wants answers memoized does so inside its own closure.

/// Returns the SVG markup of a host icon by name, such as `fa:fa-car`, or `None` when the host
/// has no icon by that name.
pub(crate) type IconResolver<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The markup of the icon `name`, or an empty string when there is no resolver or it has no such
/// icon. A label leaves room for an icon only when this is not empty.
pub(crate) fn icon_markup(icons: Option<IconResolver<'_>>, name: &str) -> String {
    icons.and_then(|resolve| resolve(name)).unwrap_or_default()
}
