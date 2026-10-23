use collections::{HashMap, HashSet};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Entity, Hsla, ImageSource, ParsedSvg,
    RenderImage, SMOOTH_SVG_SCALE_FACTOR, ScrollDelta, ScrollHandle, ScrollWheelEvent, Size,
    Stateful, StyledText, Subscription, Task, Window, img, pulsating_between, size,
};
use markdown::parser::CodeBlockKind;
use markdown::parser::MarkdownEvent;
use markdown::{CodeBlock, Markdown, MarkdownStyle};
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ui::{CopyButton, ScrollAxes, Scrollbars, TintColor, Tooltip, WithScrollbar, prelude::*};

use settings::Settings as _;
use theme_settings::ThemeSettings;

type MermaidDiagramCache = HashMap<DiagramContents, Arc<CachedMermaidDiagram>>;

const MERMAID_MAX_ZOOM: f32 = 2.0;
/// Zoom levels within this distance of 1.0 snap back to exactly 1.0 so users
/// can easily return to the default size.
const MERMAID_ZOOM_SNAP_TOLERANCE: f32 = 0.05;
const MERMAID_ZOOM_DEBOUNCE: Duration = Duration::from_millis(300);
/// Per scroll tick, zoom changes by 10 percentage points, regardless of how
/// large a delta the platform reports for the tick.
const MERMAID_ZOOM_STEP: f32 = 0.1;
/// How many pixels of precise (trackpad) scroll make up one zoom tick.
const PIXELS_PER_ZOOM_TICK: f32 = 20.0;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DiagramContents {
    contents: SharedString,
    scale: u32,
}

/// The mermaid diagrams of a markdown document: their rendered rasters, cached
/// by contents, and their view state (tab, zoom, scroll), keyed by the source
/// offset of their code block.
pub(crate) struct MermaidDiagrams {
    /// The parse the diagrams were last synced against.
    synced_events: Option<Arc<[(Range<usize>, MarkdownEvent)]>>,
    diagrams: BTreeMap<usize, DiagramContents>,
    cache: MermaidDiagramCache,
    /// Diagram contents in source order, used to show the previous raster of
    /// an edited diagram while its new raster renders.
    order: Vec<DiagramContents>,
    views: HashMap<usize, ViewState>,
    _theme_subscription: Subscription,
}

/// Per-diagram view state, keyed by source offset in [`MermaidDiagrams::views`].
struct ViewState {
    /// Whether the source code is shown instead of the rendered diagram.
    showing_code: bool,
    /// The display scale relative to the diagram's natural size; 1.0 is 1:1.
    zoom: f32,
    /// Whether the user zoomed out to the fit-to-width floor. While set, the
    /// zoom tracks the container width so the diagram stays fully visible
    /// when the container is resized, instead of keeping a stale absolute
    /// zoom computed against the old width.
    zoomed_to_fit: bool,
    /// Horizontal scroll position, used when the diagram overflows.
    scroll_handle: ScrollHandle,
    /// The pending debounced re-raster scheduled by the last zoom change.
    debounce_task: Option<Task<()>>,
}

impl ViewState {
    /// The width of the diagram's scroll container as of the last layout,
    /// if it has been laid out.
    fn container_width(&self) -> Option<Pixels> {
        Some(self.scroll_handle.bounds().size.width).filter(|width| *width > px(0.))
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            showing_code: false,
            zoom: 1.0,
            zoomed_to_fit: false,
            scroll_handle: ScrollHandle::new(),
            debounce_task: None,
        }
    }
}

struct CachedMermaidDiagram {
    render_image: Arc<OnceLock<anyhow::Result<Arc<RenderImage>>>>,
    parsed_svg: Arc<OnceLock<Arc<ParsedSvg>>>,
    /// The previous raster shown while `render_image` is pending, along with
    /// the scale it was rasterized at, so it can be displayed at the same
    /// logical size as the raster that will replace it.
    fallback_image: Option<(Arc<RenderImage>, f32)>,
    /// The scale, relative to the diagram's natural size, that `render_image`
    /// was (or is being) rasterized at.
    rasterized_scale: f32,
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
            _theme_subscription: cx.observe_global::<theme::GlobalTheme>(|this, cx| {
                this.invalidate(cx);
            }),
        }
    }

    /// Picks up the diagrams of a new parse, rendering added or edited ones
    /// and dropping the rasters and view state of removed ones.
    pub(crate) fn sync(&mut self, markdown: &Entity<Markdown>, cx: &mut Context<Self>) {
        let parsed = markdown.read(cx).parsed_markdown();
        if self
            .synced_events
            .as_ref()
            .is_some_and(|events| Arc::ptr_eq(events, parsed.events()))
        {
            return;
        }
        self.synced_events = Some(parsed.events().clone());
        self.diagrams = parsed
            .code_blocks()
            .filter_map(|block| Some((block.source_offset, diagram_contents(&block)?)))
            .collect();
        self.views
            .retain(|offset, _| self.diagrams.contains_key(offset));
        self.update_cache(cx);
    }

    /// Re-renders every diagram, e.g. after the theme changed.
    fn invalidate(&mut self, cx: &mut Context<Self>) {
        if self.diagrams.is_empty() {
            return;
        }
        self.clear_cache(cx);
        self.update_cache(cx);
        cx.notify();
    }

    fn clear_cache(&mut self, cx: &mut App) {
        for cached in self.cache.values() {
            cached.drop_images(cx);
        }
        self.cache.clear();
        self.order.clear();
    }

    fn fallback_image(
        idx: usize,
        old_order: &[DiagramContents],
        new_order_len: usize,
        cache: &MermaidDiagramCache,
    ) -> Option<(Arc<RenderImage>, f32)> {
        if old_order.len() != new_order_len {
            return None;
        }

        old_order.get(idx).and_then(|old_content| {
            cache.get(old_content).and_then(|old_cached| {
                old_cached
                    .render_image
                    .get()
                    .and_then(|result| result.as_ref().ok().cloned())
                    .map(|image| (image, old_cached.rasterized_scale))
                    .or_else(|| old_cached.fallback_image.clone())
            })
        })
    }

    fn update_cache(&mut self, cx: &mut Context<Self>) {
        let new_order: Vec<_> = self.diagrams.values().cloned().collect();
        for ((source_offset, new_content), idx) in self.diagrams.iter().zip(0..) {
            if self.cache.contains_key(new_content) {
                continue;
            }
            let fallback = Self::fallback_image(idx, &self.order, new_order.len(), &self.cache);
            // The cache is keyed by contents, so duplicate diagrams share
            // one entry; if duplicates have different zooms, the zoom of
            // the first occurrence wins.
            let zoom = self.views.get(source_offset).map_or(1.0, |view| view.zoom);
            self.cache.insert(
                new_content.clone(),
                Arc::new(CachedMermaidDiagram::new(
                    new_content.clone(),
                    zoom,
                    fallback,
                    cx,
                )),
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

    /// The natural (zoom 1.0) logical size of a diagram, if any raster of it
    /// is available to recover the size from.
    fn natural_size(&self, contents: &DiagramContents) -> Option<Size<Pixels>> {
        let cached = self.cache.get(contents)?;
        let (image, rasterized_scale) = cached
            .render_image
            .get()
            .and_then(|result| result.as_ref().ok())
            .map(|image| (image.clone(), cached.rasterized_scale))
            .or_else(|| cached.fallback_image.clone())?;
        Some(mermaid_base_size(&image, rasterized_scale))
    }

    fn is_showing_code(&self, source_offset: usize) -> bool {
        self.views
            .get(&source_offset)
            .is_some_and(|view| view.showing_code)
    }

    fn set_showing_code(
        &mut self,
        source_offset: usize,
        showing_code: bool,
        cx: &mut Context<Self>,
    ) {
        let view = self.views.entry(source_offset).or_default();
        if view.showing_code != showing_code {
            view.showing_code = showing_code;
            cx.notify();
        }
    }

    fn zoom_level(&self, source_offset: usize) -> f32 {
        self.views.get(&source_offset).map_or(1.0, |view| view.zoom)
    }

    /// The smallest zoom level for a diagram: the scale that makes it span
    /// the content width, capped at 1.0 so diagrams that already fit are
    /// never zoomed out below their natural size. Falls back to 1.0 when the
    /// diagram has no raster yet or the container hasn't been laid out.
    fn min_zoom_level(&self, source_offset: usize) -> f32 {
        let Some(contents) = self.diagrams.get(&source_offset) else {
            return 1.0;
        };
        let Some(natural_size) = self.natural_size(contents) else {
            return 1.0;
        };
        let Some(container_width) = self
            .views
            .get(&source_offset)
            .and_then(|view| view.container_width())
        else {
            return 1.0;
        };
        if natural_size.width <= container_width {
            return 1.0;
        }
        container_width / natural_size.width
    }

    fn set_zoom_level(&mut self, source_offset: usize, zoom: f32, cx: &mut Context<Self>) {
        let min_zoom = self.min_zoom_level(source_offset);
        let requested_zoom = zoom;
        let mut zoom = zoom.clamp(min_zoom, MERMAID_MAX_ZOOM);
        if (zoom - 1.0).abs() <= MERMAID_ZOOM_SNAP_TOLERANCE {
            zoom = 1.0;
        }
        // The user zoomed out to (or past) the fit-to-width floor. From here
        // on the zoom tracks the container width (see
        // `effective_zoom_level`), until the user zooms back in. A zoom
        // landing exactly at 1.0 only sticks when it was clamped, so
        // resetting to the natural size never turns tracking on.
        let zoomed_to_fit = requested_zoom < min_zoom || (zoom <= min_zoom && zoom < 1.0);

        let debounce_task = self.schedule_rerasterize(source_offset, cx);
        let view = self.views.entry(source_offset).or_default();
        view.zoom = zoom;
        view.zoomed_to_fit = zoomed_to_fit;
        view.debounce_task = Some(debounce_task);
        cx.notify();
    }

    /// The zoom level to display a diagram at, syncing a fit-to-width zoom
    /// with the current container width. Called at render time so that a
    /// fully zoomed-out diagram stays stuck to the container width when the
    /// container is resized, rather than keeping a stale absolute zoom.
    fn effective_zoom_level(&mut self, source_offset: usize, cx: &mut Context<Self>) -> f32 {
        let zoom = self.zoom_level(source_offset);
        let zoomed_to_fit = self
            .views
            .get(&source_offset)
            .is_some_and(|view| view.zoomed_to_fit);
        if !zoomed_to_fit {
            return zoom;
        }
        let min_zoom = self.min_zoom_level(source_offset);
        if (min_zoom - zoom).abs() < 0.001 {
            return zoom;
        }
        let debounce_task = self.schedule_rerasterize(source_offset, cx);
        if let Some(view) = self.views.get_mut(&source_offset) {
            view.zoom = min_zoom;
            view.debounce_task = Some(debounce_task);
        }
        min_zoom
    }

    /// Schedules a debounced re-raster of a diagram at its current zoom.
    /// Storing the returned task in `ViewState::debounce_task` replaces (and
    /// thereby cancels) the previous timer, debouncing the expensive
    /// re-raster until zoom changes settle. Until then, the existing raster
    /// is displayed scaled to the new zoom.
    fn schedule_rerasterize(&self, source_offset: usize, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MERMAID_ZOOM_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                if let Some(view) = this.views.get_mut(&source_offset) {
                    view.debounce_task = None;
                }
                this.rerasterize(source_offset, cx);
            })
            .ok();
        })
    }

    fn scroll_handle(&mut self, source_offset: usize) -> ScrollHandle {
        self.views
            .entry(source_offset)
            .or_default()
            .scroll_handle
            .clone()
    }

    /// Re-rasterizes a single diagram at exactly the scale it is displayed at
    /// (its zoom times its natural size), reusing the cached parsed SVG so
    /// that neither mermaid layout nor SVG parsing is re-run. While the new
    /// raster is pending, the previous image keeps being displayed. No-ops if
    /// the diagram is already rasterized at that scale, or is still
    /// mid-render or failed to render.
    fn rerasterize(&mut self, source_offset: usize, cx: &mut Context<Self>) {
        let Some(contents) = self.diagrams.get(&source_offset) else {
            return;
        };
        let target_scale = self.zoom_level(source_offset);
        let Some(cached) = self.cache.get_mut(contents) else {
            return;
        };
        let Some(parsed_svg) = cached.parsed_svg.get().cloned() else {
            return;
        };
        if (cached.rasterized_scale - target_scale).abs() < 0.001 {
            return;
        }
        // The old render image lives on as the new entry's fallback, so it
        // must not be dropped here; the old fallback is no longer painted.
        // If the old raster is still pending, keep showing the previous
        // fallback so the diagram doesn't disappear mid-zoom.
        let new_fallback = match cached
            .render_image
            .get()
            .and_then(|result| result.as_ref().ok())
        {
            Some(image) => {
                if let Some((old_fallback, _)) = cached.fallback_image.clone() {
                    cx.drop_image(old_fallback, None);
                }
                Some((image.clone(), cached.rasterized_scale))
            }
            None => cached.fallback_image.clone(),
        };
        let scale_factor = contents.scale as f32 / 100.0 * target_scale;
        *cached = Arc::new(CachedMermaidDiagram::new_from_parsed(
            parsed_svg,
            scale_factor,
            target_scale,
            new_fallback,
            cx,
        ));
        cx.notify();
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
        // The diagrams may lag behind the rendered parse for a frame; render
        // anything that doesn't match as a plain code block.
        let contents = self
            .diagrams
            .get(&source_offset)
            .filter(|contents| contents.contents.as_ref() == block.contents)?
            .clone();
        let showing_code = self.is_showing_code(source_offset);
        let zoom = self.effective_zoom_level(source_offset, cx);
        let this = cx.entity();
        let cached = self.cache.get(&contents);
        let code = contents.contents.clone();

        let mut container = div().group("code_block").relative().w_full().rounded_lg();
        container.style().refine(&style.code_block);

        // Prefer the current raster, then the previous one while the current
        // one renders, and show the code when neither is available.
        let raster = match cached.and_then(|cached| cached.render_image.get()) {
            Some(Ok(image)) => Some((
                image.clone(),
                cached.map_or(1.0, |cached| cached.rasterized_scale),
            )),
            Some(Err(_)) => None,
            None => cached.and_then(|cached| cached.fallback_image.clone()),
        };
        let is_rendering = cached.is_some_and(|cached| cached.render_image.get().is_none());

        let element = match raster {
            Some((image, rasterized_scale)) => {
                let body = if showing_code {
                    render_code_view(&code)
                } else {
                    let scroll_handle = self.scroll_handle(source_offset);
                    let display_size =
                        mermaid_display_size(mermaid_base_size(&image, rasterized_scale), zoom);
                    let image_element = img(ImageSource::Render(image)).with_fallback(|| {
                        Label::new("Failed to Load Mermaid Diagram").into_any_element()
                    });
                    let body = scroll_container(this.clone(), source_offset, &scroll_handle)
                        .child(image_element.w(display_size.width).h(display_size.height))
                        .into_any_element();
                    with_horizontal_scrollbar(source_offset, &scroll_handle, body, window, cx)
                };
                container
                    .child(render_tab_header(source_offset, showing_code, this.clone()))
                    .child(body)
                    .child(render_overlay_controls(
                        source_offset,
                        code,
                        (!showing_code && zoom != 1.0).then_some(zoom),
                        this,
                    ))
            }
            None => container
                .child(render_code_view(&code))
                .when(is_rendering, |container| {
                    container.child(
                        div().absolute().top_1().right_2().child(
                            Label::new("Rendering...")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .with_animation(
                                    "mermaid-loading-pulse",
                                    Animation::new(Duration::from_secs(2))
                                        .repeat()
                                        .with_easing(pulsating_between(0.4, 0.8)),
                                    |label, delta| label.alpha(delta),
                                ),
                        ),
                    )
                })
                .child(render_overlay_controls(source_offset, code, None, this)),
        };
        Some(element.into_any_element())
    }
}

impl CachedMermaidDiagram {
    fn new(
        contents: DiagramContents,
        zoom: f32,
        fallback_image: Option<(Arc<RenderImage>, f32)>,
        cx: &mut Context<MermaidDiagrams>,
    ) -> Self {
        let render_image = Arc::new(OnceLock::<anyhow::Result<Arc<RenderImage>>>::new());
        let parsed_svg = Arc::new(OnceLock::<Arc<ParsedSvg>>::new());
        let svg_renderer = cx.svg_renderer();
        let theme = mermaid_theme(cx);

        let task = cx.spawn({
            let render_image = render_image.clone();
            let parsed_svg = parsed_svg.clone();
            let fallback_image = fallback_image.clone();
            async move |this, cx| {
                let value = cx
                    .background_spawn(async move {
                        let svg_string = render_to_svg(&contents.contents, &theme)?;
                        let tree = svg_renderer
                            .parse_svg(svg_string.as_bytes())
                            .map_err(|error| anyhow::anyhow!("{error}"))?;
                        let tree = Arc::new(tree);
                        let scale = contents.scale as f32 / 100.0 * zoom;
                        let image = svg_renderer
                            .render_parsed(&tree, scale)
                            .map_err(|error| anyhow::anyhow!("{error}"))?;
                        anyhow::Ok((tree, image))
                    })
                    .await;
                let value = value.map(|(tree, image)| {
                    parsed_svg.set(tree).ok();
                    image
                });
                render_image.set(value).ok();
                Self::on_render_complete(this, fallback_image, cx);
            }
        });

        Self {
            render_image,
            parsed_svg,
            fallback_image,
            rasterized_scale: zoom,
            _task: task,
        }
    }

    fn new_from_parsed(
        parsed_svg: Arc<ParsedSvg>,
        scale_factor: f32,
        rasterized_scale: f32,
        fallback_image: Option<(Arc<RenderImage>, f32)>,
        cx: &mut Context<MermaidDiagrams>,
    ) -> Self {
        let render_image = Arc::new(OnceLock::<anyhow::Result<Arc<RenderImage>>>::new());
        let parsed_svg_cell = Arc::new(OnceLock::new());
        parsed_svg_cell.set(parsed_svg.clone()).ok();
        let svg_renderer = cx.svg_renderer();

        let task = cx.spawn({
            let render_image = render_image.clone();
            let fallback_image = fallback_image.clone();
            async move |this, cx| {
                let value = cx
                    .background_spawn(async move {
                        svg_renderer
                            .render_parsed(&parsed_svg, scale_factor)
                            .map_err(|error| anyhow::anyhow!("{error}"))
                    })
                    .await;
                render_image.set(value).ok();
                Self::on_render_complete(this, fallback_image, cx);
            }
        });

        Self {
            render_image,
            parsed_svg: parsed_svg_cell,
            fallback_image,
            rasterized_scale,
            _task: task,
        }
    }

    fn on_render_complete(
        this: gpui::WeakEntity<MermaidDiagrams>,
        fallback_image: Option<(Arc<RenderImage>, f32)>,
        cx: &mut gpui::AsyncApp,
    ) {
        this.update(cx, |_, cx| {
            // The fallback will no longer be painted now that the real render
            // is available, so its GPU texture can be released.
            if let Some((fallback_image, _)) = fallback_image {
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
        if let Some((fallback_image, _)) = self.fallback_image.clone() {
            cx.drop_image(fallback_image, None);
        }
    }
}

fn render_to_svg(source: &str, theme: &mermaid::Theme) -> anyhow::Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |now| now.as_millis() as f64);
    // The raster has a fixed size, so the SVG must not scale to a container.
    mermaid::render(source, r#"{"responsive":false}"#, now, theme)
        .map(|rendered| rendered.svg)
        .map_err(|error| anyhow::anyhow!("{error:?}"))
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
            match Path::new(path_range.path.as_ref())
                .extension()
                .and_then(|ext| ext.to_str())
            {
                Some("mermaid" | "mmd") => 100,
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(DiagramContents {
        contents: block.contents.to_string().into(),
        scale,
    })
}

fn parse_mermaid_info(info: &str) -> Option<u32> {
    let mut parts = info.split_whitespace();
    if parts.next()? != "mermaid" {
        return None;
    }

    Some(
        parts
            .next()
            .and_then(|scale| scale.parse().ok())
            .unwrap_or(100)
            .clamp(10, 500),
    )
}

/// The natural (zoom 1.0, unfitted) logical size of a diagram, recovered from
/// one of its rasters.
///
/// Mermaid rasters always come from `SvgRenderer::render_parsed`, whose images
/// have a device scale of [`SMOOTH_SVG_SCALE_FACTOR`]; dividing by it and by
/// the scale the raster was made at recovers the natural size, regardless of
/// which raster is currently cached.
fn mermaid_base_size(image: &RenderImage, rasterized_scale: f32) -> Size<Pixels> {
    let device_size = image.size(0);
    let device_scale = SMOOTH_SVG_SCALE_FACTOR * rasterized_scale;
    size(
        px(device_size.width.0 as f32 / device_scale),
        px(device_size.height.0 as f32 / device_scale),
    )
}

fn mermaid_display_size(base_size: Size<Pixels>, display_scale: f32) -> Size<Pixels> {
    size(
        base_size.width * display_scale,
        base_size.height * display_scale,
    )
}

/// The number of zoom ticks represented by a scroll-wheel event.
///
/// A discrete wheel notch arrives as one `Lines` event whose magnitude varies
/// by platform and device (1 or 3 lines are both common), so the delta is
/// clamped to a single tick: every notch is exactly one step. Precise (pixel)
/// deltas from trackpads arrive as many small events, each contributing a
/// fractional tick, so a gesture zooms smoothly at the same overall rate.
fn mermaid_zoom_ticks(delta: ScrollDelta) -> f32 {
    match delta {
        ScrollDelta::Lines(lines) => lines.y,
        ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / PIXELS_PER_ZOOM_TICK,
    }
    .clamp(-1.0, 1.0)
}

fn on_zoom_scroll(
    diagrams: Entity<MermaidDiagrams>,
    source_offset: usize,
) -> impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static {
    move |event, _window, cx| {
        if !(event.modifiers.control || event.modifiers.platform) {
            return;
        }
        let scroll_ticks = mermaid_zoom_ticks(event.delta);
        if scroll_ticks != 0.0 {
            diagrams.update(cx, |diagrams, cx| {
                let new_zoom =
                    diagrams.zoom_level(source_offset) + scroll_ticks * MERMAID_ZOOM_STEP;
                diagrams.set_zoom_level(source_offset, new_zoom, cx);
            });
        }
        cx.stop_propagation();
    }
}

/// The horizontal scroll container wrapping a mermaid raster. The element id
/// and the [`ScrollHandle`] identity are both stable per source offset so the
/// scroll position survives raster swaps and re-rasters. Diagrams keep their
/// natural (zoomed) size and scroll instead of being crushed via max_w_full
/// (#61051).
fn scroll_container(
    diagrams: Entity<MermaidDiagrams>,
    source_offset: usize,
    scroll_handle: &ScrollHandle,
) -> Stateful<Div> {
    let mut container = div()
        .id(ElementId::named_usize(
            "mermaid-diagram-body",
            source_offset,
        ))
        .w_full()
        .overflow_x_scroll()
        .track_scroll(scroll_handle)
        .on_scroll_wheel(on_zoom_scroll(diagrams, source_offset));
    // Without this, gpui maps vertical wheel deltas onto the x axis for
    // x-only scroll containers (see `paint_scroll_listener` in gpui's div),
    // hijacking plain vertical scrolls. Restricting to the actual axis lets
    // vertical deltas propagate to the surrounding view, and means a
    // ctrl+vertical-scroll (zoom gesture) never moves this container either,
    // since its delta.x is zero.
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
    // zoomed diagram can be scrolled. It only appears when the content
    // actually overflows, since the thumb isn't drawn when there's nothing to
    // scroll.
    let scrollbars = Scrollbars::always_visible(ScrollAxes::Horizontal)
        .id(("mermaid-diagram-scrollbar", source_offset))
        .tracked_scroll_handle(scroll_handle)
        .with_track_along(
            ScrollAxes::Horizontal,
            cx.theme().colors().editor_background,
        )
        .notify_content();

    div()
        .w_full()
        .custom_scrollbars(scrollbars, window, cx)
        .child(body)
        .into_any_element()
}

fn render_tab_header(
    source_offset: usize,
    showing_code: bool,
    diagrams: Entity<MermaidDiagrams>,
) -> impl IntoElement {
    let tab = |name: &'static str, label: &'static str, selected: bool, show_code: bool| {
        let diagrams = diagrams.clone();
        Button::new(ElementId::named_usize(name, source_offset), label)
            .label_size(LabelSize::Small)
            .selected_style(ButtonStyle::Tinted(TintColor::Accent))
            .toggle_state(selected)
            .on_click(move |_event, _window, cx| {
                diagrams.update(cx, |diagrams, cx| {
                    diagrams.set_showing_code(source_offset, show_code, cx)
                });
            })
    };

    h_flex()
        .gap_0p5()
        .mb_2p5()
        .child(tab("mermaid-tab-preview", "Preview", !showing_code, false))
        .child(tab("mermaid-tab-code", "Code", showing_code, true))
}

/// The overlay controls anchored to the top-right corner of a diagram: an
/// optional "Zoom NNN%" readout with a reset button (shown only while zoomed
/// away from the natural size) followed by the hover-revealed copy button.
fn render_overlay_controls(
    source_offset: usize,
    code: SharedString,
    zoom: Option<f32>,
    diagrams: Entity<MermaidDiagrams>,
) -> impl IntoElement {
    h_flex()
        .absolute()
        .top_1()
        .right_1()
        .gap_0p5()
        .when_some(zoom, |this, zoom| {
            this.child(render_zoom_indicator(source_offset, zoom, diagrams))
        })
        .child(
            CopyButton::new(
                ElementId::named_usize("copy-mermaid-code", source_offset),
                code,
            )
            .visible_on_hover("code_block"),
        )
}

/// A "Zoom NNN%" readout paired with a reset button, styled like the adjacent
/// copy button. Shown only while the diagram is zoomed away from its natural
/// size, so it doubles as an affordance that the zoom can be reset.
fn render_zoom_indicator(
    source_offset: usize,
    zoom: f32,
    diagrams: Entity<MermaidDiagrams>,
) -> impl IntoElement {
    let percentage = (zoom * 100.0).round() as i32;

    h_flex()
        .gap_0p5()
        .child(
            Label::new(format!("Zoom {percentage}%"))
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            IconButton::new(
                ElementId::named_usize("mermaid-zoom-reset", source_offset),
                IconName::RotateCcw,
            )
            .icon_size(IconSize::Small)
            .icon_color(Color::Muted)
            .tooltip(Tooltip::text("Reset Zoom"))
            .on_click(move |_event, _window, cx| {
                diagrams.update(cx, |diagrams, cx| {
                    diagrams.set_zoom_level(source_offset, 1.0, cx)
                });
            }),
        )
}

fn render_code_view(contents: &SharedString) -> AnyElement {
    div()
        .w_full()
        .child(StyledText::new(contents.clone()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_themed_svg_without_css_functions() {
        let theme = mermaid::Theme {
            foreground: mermaid::Color::rgb(0xd4, 0xd4, 0xd4),
            ..mermaid::Theme::monochrome(
                mermaid::Color::rgb(0x1e, 0x1e, 0x1e),
                mermaid::Color::rgb(0xd4, 0xd4, 0xd4),
            )
        };
        let svg = render_to_svg(
            "flowchart LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]",
            &theme,
        )
        .unwrap();
        assert!(!svg.contains("var("), "{svg}");
        assert!(!svg.contains("color-mix("), "{svg}");
        assert!(svg.contains("#d4d4d4"));
    }
}
