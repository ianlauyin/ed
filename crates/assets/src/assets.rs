// This crate was essentially pulled out verbatim from main `ed` crate to avoid having to run RustEmbed macro whenever ed has to be rebuilt. It saves a second or two on an incremental build.

use anyhow::Context as _;
use gpui::AssetSource;
use gpui::Result;
use gpui::SharedString;

// Release builds embed the assets; dev builds read them from the checkout at
// runtime so edits show up on the next launch without a rebuild and no
// build-time path is baked in (which corgi's sandbox rejects). See
// `util::fs_embed!`.
util::fs_embed! {
    pub struct Assets,
    crate_relative = "../../assets",
    root_relative = "assets",
    include = [
        "icons/**/*",
        "themes/**/*",
    ],
    exclude = ["themes/src/*", "*.DS_Store"],
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Self::get(path).map(|f| Some(f.data)).with_context(|| format!("loading asset at path {path:?}"))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter().filter_map(|p| if p.starts_with(path) { Some(p.into()) } else { None }).collect())
    }
}
