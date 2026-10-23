use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use collections::HashMap;
use collections::HashSet;
use gpui::Animation;
use gpui::AnimationExt;
use gpui::AnyElement;
use gpui::App;
use gpui::Context;
use gpui::Entity;
use gpui::Hsla;
use gpui::ImageSource;
use gpui::RenderImage;
use gpui::SMOOTH_SVG_SCALE_FACTOR;
use gpui::ScrollHandle;
use gpui::Size;
use gpui::Stateful;
use gpui::StyledText;
use gpui::Subscription;
use gpui::Task;
use gpui::Window;
use gpui::img;
use gpui::pulsating_between;
use gpui::size;
use markdown::CodeBlock;
use markdown::Markdown;
use markdown::MarkdownStyle;
use markdown::parser::CodeBlockKind;
use markdown::parser::MarkdownEvent;
use settings::Settings as _;
use theme_settings::ThemeSettings;
use ui::CopyButton;
use ui::ScrollAxes;
use ui::Scrollbars;
use ui::TintColor;
use ui::WithScrollbar;
use ui::prelude::*;

type MermaidDiagramCache = HashMap<DiagramContents, Arc<CachedMermaidDiagram>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DiagramContents {
    contents: SharedString,
    scale: u32,
}

/// The mermaid diagrams of a markdown document: their rendered rasters, cached
/// by contents, and their view state (tab, scroll), keyed by the source
/// offset of their code block.
pub(crate) struct MermaidDiagrams {
    /// The parse the diagrams were last synced against.
    #[allow(clippy::type_complexity)]
    synced_events: Option<Arc<[(Range<usize>, MarkdownEvent)]>>,
    diagrams: BTreeMap<usize, DiagramContents>,
    cache: MermaidDiagramCache,
    /// Diagram contents in source order, used to show the previous raster of
    /// an edited diagram while its new raster renders.
    order: Vec<DiagramContents>,
    views: HashMap<usize, ViewState>,
    /// The preview's temporary font zoom (cmd +/-) the diagrams are rendered
    /// at; see [`preview_zoom`].
    zoom: f32,
    _theme_subscription: Subscription,
}

/// Per-diagram view state, keyed by source offset in [`MermaidDiagrams::views`].
#[derive(Default)]
struct ViewState {
    /// Whether the source code is shown instead of the rendered diagram.
    showing_code: bool,
    /// Horizontal scroll position, used when the diagram overflows.
    scroll_handle: ScrollHandle,
}

struct CachedMermaidDiagram {
    render_image: Arc<OnceLock<anyhow::Result<Arc<RenderImage>>>>,
    /// The previous raster shown while `render_image` is pending.
    fallback_image: Option<Arc<RenderImage>>,
    _task: Task<()>,
}

impl MermaidDiagrams {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            synced_events: None,
            diagrams: BTreeMap::default(),
            cache: HashMap::default(),
            order: Vec::new(),
            views: HashMap::default(),
            zoom: preview_zoom(cx),
            _theme_subscription: cx.observe_global::<theme::GlobalTheme>(|this, cx| {
                this.rerender_all(cx);
                cx.notify();
            }),
        }
    }

    /// Picks up the diagrams of a new parse, rendering added or edited ones
    /// and dropping the rasters and view state of removed ones.
    pub(crate) fn sync(&mut self, markdown: &Entity<Markdown>, cx: &mut Context<Self>) {
        let parsed = markdown.read(cx).parsed_markdown();
        if self.synced_events.as_ref().is_some_and(|events| Arc::ptr_eq(events, parsed.events())) {
            return;
        }
        self.synced_events = Some(parsed.events().clone());
        self.diagrams =
            parsed.code_blocks().filter_map(|block| Some((block.source_offset, diagram_contents(&block)?))).collect();
        self.views.retain(|offset, _| self.diagrams.contains_key(offset));
        self.update_cache(cx);
    }

    /// Re-renders every diagram, after the theme or zoom changed, showing
    /// each one's current raster until its new one is ready.
    fn rerender_all(&mut self, cx: &Context<Self>) {
        for (contents, cached) in &mut self.cache {
            let fallback = cached.latest_image();
            *cached = Arc::new(CachedMermaidDiagram::new(contents.clone(), self.zoom, fallback, cx));
        }
    }

    fn set_zoom(&mut self, zoom: f32, cx: &Context<Self>) {
        if self.zoom != zoom {
            self.zoom = zoom;
            self.rerender_all(cx);
        }
    }

    fn fallback_image(
        idx: usize,
        old_order: &[DiagramContents],
        new_order_len: usize,
        cache: &MermaidDiagramCache,
    ) -> Option<Arc<RenderImage>> {
        if old_order.len() != new_order_len {
            return None;
        }

        old_order.get(idx).and_then(|old_content| cache.get(old_content)?.latest_image())
    }

    fn update_cache(&mut self, cx: &mut Context<Self>) {
        let new_order: Vec<_> = self.diagrams.values().cloned().collect();
        for (new_content, idx) in self.diagrams.values().zip(0..) {
            if self.cache.contains_key(new_content) {
                continue;
            }
            let fallback = Self::fallback_image(idx, &self.order, new_order.len(), &self.cache);
            self.cache.insert(
                new_content.clone(),
                Arc::new(CachedMermaidDiagram::new(new_content.clone(), self.zoom, fallback, cx)),
            );
        }

        let new_order_set: HashSet<_> = new_order.iter().cloned().collect();
        self.cache.retain(|content, cached| {
            let keep = new_order_set.contains(content);
            if !keep {
                cached.drop_images(cx);
            }
            keep
        });
        self.order = new_order;
    }

    fn is_showing_code(&self, source_offset: usize) -> bool {
        self.views.get(&source_offset).is_some_and(|view| view.showing_code)
    }

    fn set_showing_code(&mut self, source_offset: usize, showing_code: bool, cx: &mut Context<Self>) {
        let view = self.views.entry(source_offset).or_default();
        if view.showing_code != showing_code {
            view.showing_code = showing_code;
            cx.notify();
        }
    }

    fn scroll_handle(&mut self, source_offset: usize) -> ScrollHandle {
        self.views.entry(source_offset).or_default().scroll_handle.clone()
    }

    /// Renders `block` as a diagram, or returns `None` if it isn't one.
    pub(crate) fn render(
        &mut self,
        block: &CodeBlock,
        style: &MarkdownStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let source_offset = block.source_offset;
        self.set_zoom(preview_zoom(cx), cx);
        // The diagrams may lag behind the rendered parse for a frame; render
        // anything that doesn't match as a plain code block.
        let contents =
            self.diagrams.get(&source_offset).filter(|contents| contents.contents.as_ref() == block.contents)?.clone();
        let showing_code = self.is_showing_code(source_offset);
        let this = cx.entity();
        let cached = self.cache.get(&contents);
        let code = contents.contents.clone();

        let mut container = div().group("code_block").relative().w_full().rounded_lg();
        container.style().refine(&style.code_block);

        // Prefer the current raster, then the previous one while the current
        // one renders, and show the code when neither is available.
        let raster = match cached.and_then(|cached| cached.render_image.get()) {
            Some(Ok(image)) => Some(image.clone()),
            Some(Err(_)) => None,
            None => cached.and_then(|cached| cached.fallback_image.clone()),
        };
        let is_rendering = cached.is_some_and(|cached| cached.render_image.get().is_none());

        let element = match raster {
            Some(image) => {
                let body = if showing_code {
                    render_code_view(&code)
                } else {
                    let scroll_handle = self.scroll_handle(source_offset);
                    let display_size = mermaid_display_size(&image);
                    let image_element = img(ImageSource::Render(image))
                        .with_fallback(|| Label::new("Failed to Load Mermaid Diagram").into_any_element());
                    let body = scroll_container(source_offset, &scroll_handle)
                        .child(image_element.w(display_size.width).h(display_size.height))
                        .into_any_element();
                    with_horizontal_scrollbar(source_offset, &scroll_handle, body, window, cx)
                };
                container
                    .child(render_tab_header(source_offset, showing_code, this))
                    .child(body)
                    .child(render_copy_button(source_offset, code))
            }
            None => container
                .child(render_code_view(&code))
                .when(is_rendering, |container| {
                    container.child(div().absolute().top_1().right_2().child(
                        Label::new("Rendering...").size(LabelSize::XSmall).color(Color::Muted).with_animation(
                            "mermaid-loading-pulse",
                            Animation::new(Duration::from_secs(2)).repeat().with_easing(pulsating_between(0.4, 0.8)),
                            |label, delta| label.alpha(delta),
                        ),
                    ))
                })
                .child(render_copy_button(source_offset, code)),
        };
        Some(element.into_any_element())
    }
}

impl CachedMermaidDiagram {
    fn new(
        contents: DiagramContents,
        zoom: f32,
        fallback_image: Option<Arc<RenderImage>>,
        cx: &Context<MermaidDiagrams>,
    ) -> Self {
        let render_image = Arc::new(OnceLock::<anyhow::Result<Arc<RenderImage>>>::new());
        let svg_renderer = cx.svg_renderer();
        let theme = mermaid_theme(cx);

        let task = cx.spawn({
            let render_image = render_image.clone();
            let fallback_image = fallback_image.clone();
            async move |this, cx| {
                let value = cx
                    .background_spawn(async move {
                        let svg_string = render_to_svg(&contents.contents, &theme)?;
                        let tree = svg_renderer
                            .parse_svg(svg_string.as_bytes())
                            .map_err(|error| anyhow::anyhow!("{error}"))?;
                        svg_renderer
                            .render_parsed(&tree, contents.scale as f32 / 100.0 * zoom)
                            .map_err(|error| anyhow::anyhow!("{error}"))
                    })
                    .await;
                render_image.set(value).ok();
                Self::on_render_complete(this, fallback_image, cx);
            }
        });

        Self { render_image, fallback_image, _task: task }
    }

    /// The rendered raster, or the previous one while it is pending.
    fn latest_image(&self) -> Option<Arc<RenderImage>> {
        match self.render_image.get() {
            Some(result) => result.as_ref().ok().cloned(),
            None => self.fallback_image.clone(),
        }
    }

    fn on_render_complete(
        this: gpui::WeakEntity<MermaidDiagrams>,
        fallback_image: Option<Arc<RenderImage>>,
        cx: &mut gpui::AsyncApp,
    ) {
        this.update(cx, |_, cx| {
            // The fallback will no longer be painted now that the real render
            // is available, so its GPU texture can be released.
            if let Some(fallback_image) = fallback_image {
                cx.drop_image(fallback_image, None);
            }
            cx.notify();
        })
        .ok();
    }

    fn drop_images(&self, cx: &mut App) {
        if let Some(Ok(render_image)) = self.render_image.get() {
            cx.drop_image(render_image.clone(), None);
        }
        if let Some(fallback_image) = self.fallback_image.clone() {
            cx.drop_image(fallback_image, None);
        }
    }
}

fn render_to_svg(source: &str, theme: &mermaid::Theme) -> anyhow::Result<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |now| now.as_millis() as f64);
    let options = mermaid::Options { now, ..mermaid::Options::default() };
    mermaid::render(source, &options, theme).map(|rendered| rendered.svg).map_err(|error| anyhow::anyhow!("{error:?}"))
}

/// How far the preview font is zoomed (cmd +/-) from its configured size.
/// Diagrams scale with it, so at the configured size they render at 1:1.
fn preview_zoom(cx: &App) -> f32 {
    let settings = ThemeSettings::get_global(cx);
    let configured = theme_settings::clamp_font_size(
        settings.markdown_preview_font_size_settings().unwrap_or(settings.ui_font_size_settings()),
    );
    settings.markdown_preview_font_size(cx) / configured
}

/// The editor theme as a diagram theme.
fn mermaid_theme(cx: &App) -> mermaid::Theme {
    let colors = cx.theme().colors();
    let background = colors.editor_background;
    // Flattened onto the background, as diagram colors are opaque.
    let color = |color: Hsla| {
        let color = background.blend(color).to_rgb();
        let channel = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
        mermaid::Color::rgb(channel(color.r), channel(color.g), channel(color.b))
    };
    let accents = cx.theme().accents();
    let font_family = ThemeSettings::get_global(cx).mermaid_font_family();
    mermaid::Theme {
        background: color(background),
        foreground: color(colors.text),
        muted: color(colors.text_muted),
        line: color(colors.text_muted),
        border: color(colors.text_muted),
        surface: color(colors.element_background),
        accent: color(colors.text_accent),
        critical: color(cx.theme().status().error),
        series: std::array::from_fn(|index| color(accents.color_for_index(index as u32))),
        // Fall back to common monospace fonts when the configured one isn't
        // installed.
        font_family: format!(
            "'{}', ui-monospace, SFMono-Regular, Menlo, Consolas, 'Liberation Mono', 'DejaVu Sans Mono', monospace",
            gpui::font_name_with_fallbacks(font_family, "monospace")
        ),
        stroke_width: 1.5,
        corner_radius: 4.0,
        ..mermaid::Theme::monochrome(color(background), color(colors.text))
    }
}

/// The contents of `block` if it is a mermaid diagram: a
/// ```` ```mermaid [scale%] ```` fence, or a fence naming a `.mermaid` or
/// `.mmd` file.
fn diagram_contents(block: &CodeBlock) -> Option<DiagramContents> {
    let scale = match block.kind {
        CodeBlockKind::FencedLang(info) => parse_mermaid_info(info)?,
        CodeBlockKind::FencedSrc(path_range) => {
            match Path::new(path_range.path.as_ref()).extension().and_then(|ext| ext.to_str()) {
                Some("mermaid" | "mmd") => 100,
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(DiagramContents { contents: block.contents.to_string().into(), scale })
}

fn parse_mermaid_info(info: &str) -> Option<u32> {
    let mut parts = info.split_whitespace();
    if parts.next()? != "mermaid" {
        return None;
    }

    Some(parts.next().and_then(|scale| scale.parse().ok()).unwrap_or(100).clamp(10, 500))
}

/// The logical size of a diagram's raster.
///
/// Mermaid rasters come from `SvgRenderer::render_parsed`, whose images have a
/// device scale of [`SMOOTH_SVG_SCALE_FACTOR`].
fn mermaid_display_size(image: &RenderImage) -> Size<Pixels> {
    let device_size = image.size(0);
    size(
        px(device_size.width.0 as f32 / SMOOTH_SVG_SCALE_FACTOR),
        px(device_size.height.0 as f32 / SMOOTH_SVG_SCALE_FACTOR),
    )
}

/// The horizontal scroll container wrapping a mermaid raster. The element id
/// and the [`ScrollHandle`] identity are both stable per source offset so the
/// scroll position survives raster swaps. Diagrams keep their natural size and
/// scroll instead of being crushed via max_w_full (#61051).
fn scroll_container(source_offset: usize, scroll_handle: &ScrollHandle) -> Stateful<Div> {
    let mut container = div()
        .id(ElementId::named_usize("mermaid-diagram-body", source_offset))
        .w_full()
        .overflow_x_scroll()
        .track_scroll(scroll_handle);
    // Without this, gpui maps vertical wheel deltas onto the x axis for
    // x-only scroll containers (see `paint_scroll_listener` in gpui's div),
    // hijacking plain vertical scrolls. Restricting to the actual axis lets
    // vertical deltas propagate to the surrounding view.
    container.style().restrict_scroll_to_axis = Some(true);
    container
}

fn with_horizontal_scrollbar(
    source_offset: usize,
    scroll_handle: &ScrollHandle,
    body: AnyElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    // Always show the scrollbar (rather than autohiding) so it's clear a
    // wide diagram can be scrolled. It only appears when the content
    // actually overflows, since the thumb isn't drawn when there's nothing to
    // scroll.
    let scrollbars = Scrollbars::always_visible(ScrollAxes::Horizontal)
        .id(("mermaid-diagram-scrollbar", source_offset))
        .tracked_scroll_handle(scroll_handle)
        .with_track_along(ScrollAxes::Horizontal, cx.theme().colors().editor_background)
        .notify_content();

    div().w_full().custom_scrollbars(scrollbars, window, cx).child(body).into_any_element()
}

fn render_tab_header(source_offset: usize, showing_code: bool, diagrams: Entity<MermaidDiagrams>) -> impl IntoElement {
    let tab = |name: &'static str, label: &'static str, selected: bool, show_code: bool| {
        let diagrams = diagrams.clone();
        Button::new(ElementId::named_usize(name, source_offset), label)
            .label_size(LabelSize::Small)
            .selected_style(ButtonStyle::Tinted(TintColor::Accent))
            .toggle_state(selected)
            .on_click(move |_event, _window, cx| {
                diagrams.update(cx, |diagrams, cx| diagrams.set_showing_code(source_offset, show_code, cx));
            })
    };

    h_flex().gap_0p5().mb_2p5().child(tab("mermaid-tab-preview", "Preview", !showing_code, false)).child(tab(
        "mermaid-tab-code",
        "Code",
        showing_code,
        true,
    ))
}

/// The hover-revealed copy button anchored to the top-right corner of a
/// diagram.
fn render_copy_button(source_offset: usize, code: SharedString) -> impl IntoElement {
    div().absolute().top_1().right_1().child(
        CopyButton::new(ElementId::named_usize("copy-mermaid-code", source_offset), code)
            .visible_on_hover("code_block"),
    )
}

fn render_code_view(contents: &SharedString) -> AnyElement {
    div().w_full().child(StyledText::new(contents.clone())).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_themed_svg_without_css_functions() {
        let theme = mermaid::Theme {
            foreground: mermaid::Color::rgb(0xd4, 0xd4, 0xd4),
            ..mermaid::Theme::monochrome(mermaid::Color::rgb(0x1e, 0x1e, 0x1e), mermaid::Color::rgb(0xd4, 0xd4, 0xd4))
        };
        let svg = render_to_svg("flowchart LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]", &theme).unwrap();
        assert!(!svg.contains("var("), "{svg}");
        assert!(!svg.contains("color-mix("), "{svg}");
        // usvg does not parse `normal` spacing and logs a warning per diagram.
        assert!(!svg.contains("spacing:normal"), "{svg}");
        assert!(svg.contains("#d4d4d4"));
    }
}
