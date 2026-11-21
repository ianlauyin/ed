use std::sync::Arc;

use anyhow::Context as _;
use gpui::Action;
use gpui::AnyView;
use gpui::App;
use gpui::AppContext as _;
use gpui::Axis;
use gpui::Context;
use gpui::Entity;
use gpui::EntityId;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::IntoElement;
use gpui::KeyContext;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::MouseUpEvent;
use gpui::ParentElement as _;
use gpui::Render;
use gpui::SharedString;
use gpui::StyleRefinement;
use gpui::Styled as _;
use gpui::Subscription;
use gpui::WeakEntity;
use gpui::Window;
use gpui::deferred;
use gpui::div;
use gpui::px;
use serde::Deserialize;
use serde::Serialize;
use ui::CountBadge;
use ui::Divider;
use ui::DividerColor;
use ui::IconButton;
use ui::Tooltip;
use ui::prelude::ActiveTheme as _;
use ui::prelude::ButtonCommon as _;
use ui::prelude::Clickable as _;
use ui::prelude::FluentBuilder as _;
use ui::prelude::IconSize;
use ui::prelude::InteractiveElement as _;
use ui::prelude::Pixels;
use ui::prelude::StatefulInteractiveElement as _;
use ui::prelude::StyledExt as _;
use ui::prelude::Toggleable as _;
use ui::prelude::h_flex;
use util::ResultExt as _;

use crate::Event;
use crate::ModalLayer;
use crate::Pane;
use crate::Workspace;
use crate::docks::DraggedDock;
use crate::persistence::model::DockData;
use crate::status_bar::StatusItemView;

pub(crate) const RESIZE_HANDLE_SIZE: Pixels = px(6.);
/// Minimum width of sidebar-style panels (project, git, outline) and the Git History sidebar.
pub const SIDEBAR_MIN_WIDTH: Pixels = px(240.);

pub enum PanelEvent {
    ZoomIn,
    ZoomOut,
    Activate,
    Close,
}

pub trait Panel: Focusable + EventEmitter<PanelEvent> + Render + Sized {
    fn persistent_name() -> &'static str;
    fn panel_key() -> &'static str;
    /// The `Focusable::focus_handle` root identifies the panel's subtree for containment checks
    /// and must be tracked by the panel's root element. This method returns the handle that should
    /// receive focus when the panel is activated, such as a filter, commit, or message editor; it
    /// must be a focus-tree descendant of the root or containment checks such as Zen-mode auto-close
    /// and toggle-focus will misbehave.
    fn activation_focus_handle(&self, cx: &App) -> FocusHandle {
        self.focus_handle(cx)
    }
    fn position(&self, window: &Window, cx: &App) -> DockPosition;
    fn default_size(&self, window: &Window, cx: &App) -> Pixels;
    fn min_size(&self, _window: &Window, _cx: &App) -> Option<Pixels> {
        None
    }
    fn initial_size_state(&self, _window: &Window, _cx: &App) -> PanelSizeState {
        PanelSizeState::default()
    }
    fn size_state_changed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn icon(&self, window: &Window, cx: &App) -> Option<ui::IconName>;
    fn icon_tooltip(&self, window: &Window, cx: &App) -> Option<&'static str>;
    fn toggle_action(&self) -> Box<dyn Action>;
    fn icon_label(&self, _window: &Window, _: &App) -> Option<String> {
        None
    }
    fn is_zoomed(&self, _window: &Window, _cx: &App) -> bool {
        false
    }
    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        false
    }
    fn set_zoomed(&mut self, _zoomed: bool, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn set_active(&mut self, _active: bool, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn pane(&self) -> Option<Entity<Pane>> {
        None
    }

    fn activation_priority(&self) -> u32;
    fn enabled(&self, _cx: &App) -> bool {
        true
    }
}

pub trait PanelHandle: Send + Sync {
    fn panel_id(&self) -> EntityId;
    fn persistent_name(&self) -> &'static str;
    fn panel_key(&self) -> &'static str;
    fn position(&self, window: &Window, cx: &App) -> DockPosition;
    fn is_zoomed(&self, window: &Window, cx: &App) -> bool;
    fn set_zoomed(&self, zoomed: bool, window: &mut Window, cx: &mut App);
    fn set_active(&self, active: bool, window: &mut Window, cx: &mut App);
    fn pane(&self, cx: &App) -> Option<Entity<Pane>>;
    fn default_size(&self, window: &Window, cx: &App) -> Pixels;
    fn min_size(&self, window: &Window, cx: &App) -> Option<Pixels>;
    fn initial_size_state(&self, window: &Window, cx: &App) -> PanelSizeState;
    fn size_state_changed(&self, window: &mut Window, cx: &mut App);
    fn icon(&self, window: &Window, cx: &App) -> Option<ui::IconName>;
    fn icon_tooltip(&self, window: &Window, cx: &App) -> Option<&'static str>;
    fn toggle_action(&self, window: &Window, cx: &App) -> Box<dyn Action>;
    fn icon_label(&self, window: &Window, cx: &App) -> Option<String>;
    fn panel_focus_handle(&self, cx: &App) -> FocusHandle;
    /// See `Panel::activation_focus_handle`.
    fn activation_focus_handle(&self, cx: &App) -> FocusHandle;
    fn to_any(&self) -> AnyView;
    fn activation_priority(&self, cx: &App) -> u32;
    fn enabled(&self, cx: &App) -> bool;
}

impl<T> PanelHandle for Entity<T>
where
    T: Panel,
{
    fn panel_id(&self) -> EntityId {
        Entity::entity_id(self)
    }

    fn persistent_name(&self) -> &'static str {
        T::persistent_name()
    }

    fn panel_key(&self) -> &'static str {
        T::panel_key()
    }

    fn position(&self, window: &Window, cx: &App) -> DockPosition {
        self.read(cx).position(window, cx)
    }

    fn is_zoomed(&self, window: &Window, cx: &App) -> bool {
        self.read(cx).is_zoomed(window, cx)
    }

    fn set_zoomed(&self, zoomed: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_zoomed(zoomed, window, cx));
    }

    fn set_active(&self, active: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_active(active, window, cx));
    }

    fn pane(&self, cx: &App) -> Option<Entity<Pane>> {
        self.read(cx).pane()
    }

    fn default_size(&self, window: &Window, cx: &App) -> Pixels {
        self.read(cx).default_size(window, cx)
    }

    fn min_size(&self, window: &Window, cx: &App) -> Option<Pixels> {
        self.read(cx).min_size(window, cx)
    }

    fn initial_size_state(&self, window: &Window, cx: &App) -> PanelSizeState {
        self.read(cx).initial_size_state(window, cx)
    }

    fn size_state_changed(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.size_state_changed(window, cx));
    }

    fn icon(&self, window: &Window, cx: &App) -> Option<ui::IconName> {
        self.read(cx).icon(window, cx)
    }

    fn icon_tooltip(&self, window: &Window, cx: &App) -> Option<&'static str> {
        self.read(cx).icon_tooltip(window, cx)
    }

    fn toggle_action(&self, _: &Window, cx: &App) -> Box<dyn Action> {
        self.read(cx).toggle_action()
    }

    fn icon_label(&self, window: &Window, cx: &App) -> Option<String> {
        self.read(cx).icon_label(window, cx)
    }

    fn to_any(&self) -> AnyView {
        self.clone().into()
    }

    fn panel_focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn activation_focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).activation_focus_handle(cx)
    }

    fn activation_priority(&self, cx: &App) -> u32 {
        self.read(cx).activation_priority()
    }

    fn enabled(&self, cx: &App) -> bool {
        self.read(cx).enabled(cx)
    }
}

impl From<&dyn PanelHandle> for AnyView {
    fn from(val: &dyn PanelHandle) -> Self {
        val.to_any()
    }
}

/// A container with a fixed [`DockPosition`] adjacent to a certain widown edge.
/// Can contain multiple panels and show/hide itself with all contents.
pub struct Dock {
    position: DockPosition,
    panel_entries: Vec<PanelEntry>,
    workspace: WeakEntity<Workspace>,
    is_open: bool,
    active_panel_index: Option<usize>,
    focus_handle: FocusHandle,
    restoration: DockRestoreState,
    zoom_layer_open: bool,
    modal_layer: Entity<ModalLayer>,
    _subscriptions: [Subscription; 2],
}

enum DockRestoreState {
    Restoring { pending: Option<DockData> },
    Finished,
}

impl DockRestoreState {
    const fn pending(&self) -> Option<&DockData> {
        match self {
            Self::Restoring { pending } => pending.as_ref(),
            Self::Finished => None,
        }
    }

    fn discard_pending(&mut self) {
        if let Self::Restoring { pending } = self {
            *pending = None;
        }
    }
}

impl Focusable for Dock {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DockPosition {
    Left,
    Bottom,
}

impl DockPosition {
    const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Bottom => "Bottom",
        }
    }

    pub const fn axis(&self) -> Axis {
        match self {
            Self::Left => Axis::Horizontal,
            Self::Bottom => Axis::Vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSizeState {
    pub size: Option<Pixels>,
}

struct PanelEntry {
    panel: Arc<dyn PanelHandle>,
    size_state: PanelSizeState,
    _subscriptions: [Subscription; 2],
}

pub struct PanelButtons {
    dock: Entity<Dock>,
}

/// Keeps a resized panel at least as large as the resize handle and the panel's minimum.
fn clamp_resized_panel_size(size: Pixels, min_size: Option<Pixels>) -> Pixels {
    size.max(RESIZE_HANDLE_SIZE).max(min_size.unwrap_or(Pixels::ZERO)).round()
}

fn resize_panel_entry(entry: &mut PanelEntry, size: Option<Pixels>, axis: Axis, window: &mut Window, cx: &mut App) {
    // `Panel::min_size` is a width; it doesn't apply to the bottom dock.
    let min_size = if axis == Axis::Horizontal { entry.panel.min_size(window, cx) } else { None };
    entry.size_state.size = size.map(|size| clamp_resized_panel_size(size, min_size));
    entry.panel.size_state_changed(window, cx);
}

impl Dock {
    pub fn new(
        position: DockPosition,
        modal_layer: Entity<ModalLayer>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let focus_handle = cx.focus_handle();
        let workspace = cx.entity();
        let dock = cx.new(|cx| {
            let focus_subscription = cx.on_focus(&focus_handle, window, |dock: &mut Dock, window, cx| {
                if let Some(active_entry) = dock.active_panel_entry() {
                    active_entry.panel.activation_focus_handle(cx).focus(window, cx);
                }
            });
            let zoom_subscription = cx.subscribe(&workspace, |dock, workspace, e: &Event, cx| {
                if matches!(e, Event::ZoomChanged) {
                    let is_zoomed = workspace.read(cx).zoomed.is_some();
                    dock.zoom_layer_open = is_zoomed;
                }
            });
            Self {
                position,
                workspace: workspace.downgrade(),
                panel_entries: Vec::default(),
                active_panel_index: None,
                is_open: false,
                focus_handle: focus_handle.clone(),
                _subscriptions: [focus_subscription, zoom_subscription],
                restoration: DockRestoreState::Restoring { pending: None },
                zoom_layer_open: false,
                modal_layer,
            }
        });

        cx.on_focus_in(&focus_handle, window, {
            let dock = dock.downgrade();
            move |workspace, window, cx| {
                let Some(dock) = dock.upgrade() else {
                    return;
                };
                let Some(panel) = dock.read(cx).active_panel() else {
                    return;
                };
                if panel.is_zoomed(window, cx) {
                    workspace.zoomed = Some(panel.to_any().downgrade());
                    workspace.zoomed_position = Some(position);
                } else {
                    workspace.zoomed = None;
                    workspace.zoomed_position = None;
                }
                cx.emit(Event::ZoomChanged);
                workspace.dismiss_zoomed_items_to_reveal(Some(position), window, cx);
            }
        })
        .detach();

        cx.observe_in(&dock, window, move |workspace, dock, window, cx| {
            if dock.read(cx).is_open()
                && let Some(panel) = dock.read(cx).active_panel()
                && panel.is_zoomed(window, cx)
            {
                workspace.zoomed = Some(panel.to_any().downgrade());
                workspace.zoomed_position = Some(position);
                cx.emit(Event::ZoomChanged);
                return;
            }
            if workspace.zoomed_position == Some(position) {
                workspace.zoomed = None;
                workspace.zoomed_position = None;
                cx.emit(Event::ZoomChanged);
            }
        })
        .detach();

        dock
    }

    pub const fn position(&self) -> DockPosition {
        self.position
    }

    pub const fn is_open(&self) -> bool {
        self.is_open
    }

    fn resizable(&self, cx: &App) -> bool {
        !(self.zoom_layer_open || self.modal_layer.read(cx).has_active_modal())
    }

    pub fn panel<T: Panel>(&self) -> Option<Entity<T>> {
        self.panel_entries.iter().find_map(|entry| entry.panel.to_any().downcast().ok())
    }

    pub fn panel_index_for_type<T: Panel>(&self) -> Option<usize> {
        self.panel_entries.iter().position(|entry| entry.panel.to_any().downcast::<T>().is_ok())
    }

    pub fn panel_index_for_persistent_name(&self, ui_name: &str, _cx: &App) -> Option<usize> {
        self.panel_entries.iter().position(|entry| entry.panel.persistent_name() == ui_name)
    }

    pub fn panels_enabled(&self, cx: &App) -> Vec<bool> {
        self.panel_entries.iter().map(|entry| entry.panel.enabled(cx)).collect()
    }

    pub fn first_enabled_panel_idx(&mut self, cx: &mut Context<Self>) -> anyhow::Result<usize> {
        self.panel_entries
            .iter()
            .position(|entry| entry.panel.enabled(cx))
            .with_context(|| format!("Couldn't find any enabled panel for the {} dock.", self.position.label()))
    }

    fn active_panel_entry(&self) -> Option<&PanelEntry> {
        let index = self.active_panel_index?;
        self.panel_entries.get(index)
    }

    fn active_panel_entry_mut(&mut self) -> Option<&mut PanelEntry> {
        let index = self.active_panel_index?;
        self.panel_entries.get_mut(index)
    }

    pub const fn active_panel_index(&self) -> Option<usize> {
        self.active_panel_index
    }

    pub fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open != self.is_open {
            self.restoration.discard_pending();
        }
        self.set_open_internal(open, window, cx);
    }

    fn set_open_internal(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open != self.is_open {
            self.is_open = open;
            if let Some(active_panel) = self.active_panel_entry() {
                active_panel.panel.set_active(open, window, cx);
            }

            cx.notify();
        }
    }

    pub fn set_panel_zoomed(&mut self, panel: &AnyView, zoomed: bool, window: &mut Window, cx: &mut Context<Self>) {
        for entry in &mut self.panel_entries {
            if entry.panel.panel_id() == panel.entity_id() {
                if zoomed != entry.panel.is_zoomed(window, cx) {
                    entry.panel.set_zoomed(zoomed, window, cx);
                }
            } else if entry.panel.is_zoomed(window, cx) {
                entry.panel.set_zoomed(false, window, cx);
            }
        }

        self.workspace
            .update(cx, |workspace, cx| {
                workspace.serialize_workspace(window, cx);
            })
            .ok();
        cx.notify();
    }

    pub fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for entry in &mut self.panel_entries {
            if entry.panel.is_zoomed(window, cx) {
                entry.panel.set_zoomed(false, window, cx);
            }
        }
    }

    pub(crate) fn add_panel<T: Panel>(
        &mut self,
        panel: Entity<T>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let subscriptions = [
            cx.observe(&panel, |_, _, cx| cx.notify()),
            cx.subscribe_in(&panel, window, move |this, panel, event, window, cx| match event {
                PanelEvent::ZoomIn => {
                    this.set_panel_zoomed(&panel.to_any(), true, window, cx);
                    if !PanelHandle::panel_focus_handle(panel, cx).contains_focused(window, cx) {
                        window.focus(&panel.read(cx).activation_focus_handle(cx), cx);
                    }
                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.zoomed = Some(panel.downgrade().into());
                            workspace.zoomed_position = Some(panel.read(cx).position(window, cx));
                            cx.emit(Event::ZoomChanged);
                        })
                        .ok();
                }
                PanelEvent::ZoomOut => {
                    this.set_panel_zoomed(&panel.to_any(), false, window, cx);
                    workspace
                        .update(cx, |workspace, cx| {
                            if workspace.zoomed_position == Some(this.position) {
                                workspace.zoomed = None;
                                workspace.zoomed_position = None;
                                cx.emit(Event::ZoomChanged);
                            }
                            cx.notify();
                        })
                        .ok();
                }
                PanelEvent::Activate => {
                    if let Some(ix) =
                        this.panel_entries.iter().position(|entry| entry.panel.panel_id() == Entity::entity_id(panel))
                    {
                        this.activate_panel(ix, window, cx);
                        this.set_open(true, window, cx);
                        window.focus(&panel.read(cx).activation_focus_handle(cx), cx);
                    }
                }
                PanelEvent::Close => {
                    if this.visible_panel().is_some_and(|p| p.panel_id() == Entity::entity_id(panel)) {
                        this.set_open(false, window, cx);
                    }
                }
            }),
        ];

        let index = match self
            .panel_entries
            .binary_search_by_key(&panel.read(cx).activation_priority(), |entry| entry.panel.activation_priority(cx))
        {
            Ok(ix) => ix,
            Err(ix) => ix,
        };
        if let Some(active_index) = self.active_panel_index.as_mut()
            && *active_index >= index
        {
            *active_index += 1;
        }
        let size_state = panel.read(cx).initial_size_state(window, cx);

        self.panel_entries
            .insert(index, PanelEntry { panel: Arc::new(panel.clone()), size_state, _subscriptions: subscriptions });

        self.replay_pending_serialized_state(window, cx);

        if panel.read(cx).starts_open(window, cx) {
            self.activate_panel_internal(index, window, cx);
            self.set_open_internal(true, window, cx);
        }

        cx.notify();
        index
    }

    pub(crate) fn restore_serialized_state(
        &mut self,
        serialized: DockData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &mut self.restoration {
            DockRestoreState::Restoring { pending } => {
                *pending = Some(serialized);
                self.replay_pending_serialized_state(window, cx);
            }
            DockRestoreState::Finished => {
                let active_panel_missing = serialized
                    .active_panel
                    .as_deref()
                    .filter(|_| serialized.visible)
                    .is_some_and(|name| self.panel_index_for_persistent_name(name, cx).is_none());
                if !active_panel_missing {
                    self.apply_serialized_state(&serialized, window, cx);
                }
            }
        }
    }

    fn replay_pending_serialized_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(serialized) = self.restoration.pending().cloned() else {
            return;
        };
        let waiting_for_active_panel = serialized
            .active_panel
            .as_deref()
            .filter(|_| serialized.visible)
            .is_some_and(|name| self.panel_index_for_persistent_name(name, cx).is_none());
        if waiting_for_active_panel {
            self.set_open_internal(serialized.visible, window, cx);
        } else {
            self.apply_serialized_state(&serialized, window, cx);
            self.restoration.discard_pending();
        }
    }

    fn apply_serialized_state(&mut self, serialized: &DockData, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(active_panel) = serialized.active_panel.as_deref().filter(|_| serialized.visible)
            && let Some(idx) = self.panel_index_for_persistent_name(active_panel, cx)
        {
            self.activate_panel_internal(idx, window, cx);
        }
        if serialized.zoom
            && let Some(panel) = self.active_panel()
        {
            panel.set_zoomed(true, window, cx);
        }
        self.set_open_internal(serialized.visible, window, cx);
    }

    pub(crate) fn finish_restoration(&mut self) {
        self.restoration = DockRestoreState::Finished;
    }

    pub fn activate_panel(&mut self, panel_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if Some(panel_ix) != self.active_panel_index {
            self.restoration.discard_pending();
        }
        self.activate_panel_internal(panel_ix, window, cx);
    }

    fn activate_panel_internal(&mut self, panel_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if Some(panel_ix) != self.active_panel_index {
            if let Some(active_panel) = self.active_panel_entry() {
                active_panel.panel.set_active(false, window, cx);
            }

            self.active_panel_index = Some(panel_ix);
            if let Some(active_panel) = self.active_panel_entry() {
                active_panel.panel.set_active(true, window, cx);
            }

            cx.notify();
        }
    }

    pub fn visible_panel(&self) -> Option<&Arc<dyn PanelHandle>> {
        let entry = self.visible_entry()?;
        Some(&entry.panel)
    }

    pub fn active_panel(&self) -> Option<&Arc<dyn PanelHandle>> {
        let panel_entry = self.active_panel_entry()?;
        Some(&panel_entry.panel)
    }

    fn visible_entry(&self) -> Option<&PanelEntry> {
        if self.is_open { self.active_panel_entry() } else { None }
    }

    pub fn zoomed_panel(&self, window: &Window, cx: &App) -> Option<Arc<dyn PanelHandle>> {
        let entry = self.visible_entry()?;
        entry.panel.is_zoomed(window, cx).then(|| Arc::clone(&entry.panel))
    }

    /// Panel buttons focus this before dispatching their action. A zoomed panel isn't rendered inside
    /// its dock, so the dock's own handle would be outside the frame and the action would be dropped.
    pub(crate) fn panel_button_focus_handle(&self, window: &Window, cx: &App) -> FocusHandle {
        self.zoomed_panel(window, cx).map_or_else(|| self.focus_handle.clone(), |panel| panel.panel_focus_handle(cx))
    }

    pub fn stored_panel_size_state(&self, panel: &dyn PanelHandle) -> Option<PanelSizeState> {
        self.panel_entries.iter().find(|entry| entry.panel.panel_id() == panel.panel_id()).map(|entry| entry.size_state)
    }

    pub fn stored_active_panel_size(&self, window: &Window, cx: &App) -> Option<Pixels> {
        if self.is_open {
            self.active_panel_entry()
                .map(|entry| entry.size_state.size.unwrap_or_else(|| entry.panel.default_size(window, cx)))
        } else {
            None
        }
    }

    fn resize_active_panel(&mut self, size: Option<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let axis = self.position.axis();
        if let Some(entry) = self.active_panel_entry_mut() {
            resize_panel_entry(entry, size, axis, window, cx);
            cx.notify();
        }
    }

    /// Resizes the active panel, or every panel in the left dock.
    pub fn resize_panel_sizes(&mut self, size: Option<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if self.position == DockPosition::Left {
            self.resize_all_panels(size, window, cx);
        } else {
            self.resize_active_panel(size, window, cx);
        }
    }

    /// Resets the active panel, or every panel in the left dock.
    pub fn reset_panel_sizes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.position == DockPosition::Left {
            self.reset_all_panel_sizes(window, cx);
        } else {
            self.reset_active_panel_size(window, cx);
        }
    }

    fn reset_active_panel_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.resize_active_panel(PanelSizeState::default().size, window, cx);
    }

    fn reset_all_panel_sizes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active_entry) = self.active_panel_entry() else {
            return;
        };
        let size = active_entry.panel.default_size(window, cx);
        self.resize_all_panels(Some(size), window, cx);
    }

    fn resize_all_panels(&mut self, size: Option<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_panel_index.is_none() {
            return;
        }

        let axis = self.position.axis();
        for entry in &mut self.panel_entries {
            resize_panel_entry(entry, size, axis, window, cx);
        }
        cx.notify();
    }

    pub fn toggle_action(&self) -> Box<dyn Action> {
        match self.position {
            DockPosition::Left => crate::ToggleLeftDock.boxed_clone(),
            DockPosition::Bottom => crate::ToggleBottomDock.boxed_clone(),
        }
    }

    fn dispatch_context() -> KeyContext {
        let mut dispatch_context = KeyContext::new_with_defaults();
        dispatch_context.add("Dock");

        dispatch_context
    }

    pub fn clamp_panel_size(&mut self, max_size: Pixels, window: &Window, cx: &mut Context<Self>) {
        let max_size = (max_size - RESIZE_HANDLE_SIZE).abs();
        let mut clamped = false;
        for entry in &mut self.panel_entries {
            let size = entry.size_state.size.unwrap_or_else(|| entry.panel.default_size(window, cx));
            if size > max_size {
                entry.size_state.size = Some(max_size.max(RESIZE_HANDLE_SIZE));
                clamped = true;
            }
        }
        if clamped {
            cx.notify();
        }
    }
}

impl Render for Dock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dispatch_context = Self::dispatch_context();
        if let Some(entry) = self.visible_entry() {
            let position = self.position;
            let create_resize_handle = || {
                let handle = div()
                    .id("resize-handle")
                    .on_drag(DraggedDock(position), |dock, _, _, cx| {
                        cx.stop_propagation();
                        cx.new(|_| dock.clone())
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|dock, e: &MouseUpEvent, window, cx| {
                            if e.click_count == 2 {
                                dock.reset_panel_sizes(window, cx);
                                dock.workspace
                                    .update(cx, |workspace, cx| {
                                        workspace.serialize_workspace(window, cx);
                                    })
                                    .ok();
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .occlude();
                match self.position() {
                    DockPosition::Left => deferred(
                        handle
                            .absolute()
                            .right(-RESIZE_HANDLE_SIZE / 2.)
                            .top(px(0.))
                            .h_full()
                            .w(RESIZE_HANDLE_SIZE)
                            .cursor_col_resize(),
                    ),
                    DockPosition::Bottom => deferred(
                        handle
                            .absolute()
                            .top(-RESIZE_HANDLE_SIZE / 2.)
                            .left(px(0.))
                            .w_full()
                            .h(RESIZE_HANDLE_SIZE)
                            .cursor_row_resize(),
                    ),
                }
            };

            div()
                .id("dock-panel")
                .key_context(dispatch_context)
                .track_focus(&self.focus_handle(cx))
                .flex()
                .overflow_hidden()
                .map(|this| match self.position().axis() {
                    // Width and height are always set on the workspace wrapper in
                    // render_dock, so fill whatever space the wrapper provides.
                    Axis::Horizontal => this.w_full().h_full().flex_row(),
                    Axis::Vertical => this.h_full().w_full().flex_col(),
                })
                // Docks blend into the chrome; panes inside them (e.g. the terminal) draw their own cards.
                .bg(cx.theme().colors().panel_background)
                .child(
                    div()
                        .map(|this| match self.position().axis() {
                            Axis::Horizontal => this.w_full().h_full(),
                            Axis::Vertical => this.h_full().w_full(),
                        })
                        .child(entry.panel.to_any().cached(StyleRefinement::default().v_flex().size_full())),
                )
                .when(self.resizable(cx), |this| this.child(create_resize_handle()))
        } else {
            div().id("dock-panel").key_context(dispatch_context).track_focus(&self.focus_handle(cx))
        }
    }
}

impl PanelButtons {
    pub fn new(dock: Entity<Dock>, cx: &mut Context<Self>) -> Self {
        cx.observe(&dock, |_, _, cx| cx.notify()).detach();
        Self { dock }
    }
}

impl Render for PanelButtons {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock.read(cx);
        let active_index = dock.active_panel_index;
        let is_open = dock.is_open;

        let buttons: Vec<_> = dock
            .panel_entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                let icon = entry.panel.icon(window, cx)?;
                let icon_tooltip = entry
                    .panel
                    .icon_tooltip(window, cx)
                    .ok_or_else(|| anyhow::anyhow!("can't render a panel button without an icon tooltip"))
                    .log_err()?;
                let name = entry.panel.persistent_name();

                let is_active_button = Some(i) == active_index && is_open;
                let (action, tooltip) = if is_active_button {
                    let action = dock.toggle_action();

                    let tooltip: SharedString = format!("Close {} Dock", dock.position.label()).into();

                    (action, tooltip)
                } else {
                    let action = entry.panel.toggle_action(window, cx);

                    (action, icon_tooltip.into())
                };

                let focus_handle = dock.panel_button_focus_handle(window, cx);
                let icon_label = entry.panel.icon_label(window, cx);
                // Include active state in element ID to invalidate the cached
                // tooltip when panel state changes (e.g., via keyboard shortcut)
                let button = IconButton::new((name, is_active_button as u64), icon)
                    .icon_size(IconSize::Small)
                    .toggle_state(is_active_button)
                    .tab_index(0_isize)
                    .on_click({
                        let action = action.boxed_clone();
                        move |_, window, cx| {
                            window.focus(&focus_handle, cx);
                            window.dispatch_action(action.boxed_clone(), cx);
                        }
                    })
                    .tooltip(move |_window, cx| Tooltip::for_action(tooltip.clone(), &*action, cx));

                let button = div().relative().child(button).when_some(
                    icon_label.filter(|_| !is_active_button).and_then(|label| label.parse::<usize>().ok()),
                    |this, count| this.child(CountBadge::new(count)),
                );

                Some(button)
            })
            .collect();

        let has_buttons = !buttons.is_empty();

        h_flex()
            .gap_1()
            .when(has_buttons && dock.position == DockPosition::Bottom, |this| {
                this.child(Divider::vertical().color(DividerColor::Border))
            })
            .children(buttons)
            .when(has_buttons && dock.position == DockPosition::Left, |this| {
                this.child(Divider::vertical().color(DividerColor::Border))
            })
    }
}

impl StatusItemView for PanelButtons {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn crate::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Nothing to do, panel buttons don't depend on the active center item
    }
}

/// The panel a dock-cycle press shows next, given which panels are enabled and
/// which one is visible (`None` when the dock is hidden). `None` means close.
pub fn next_dock_cycle_panel(enabled: &[bool], visible_index: Option<usize>) -> Option<usize> {
    let start = visible_index.map_or(0, |index| index + 1);
    enabled.iter().enumerate().skip(start).find_map(|(index, enabled)| enabled.then_some(index))
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::RESIZE_HANDLE_SIZE;
    use super::SIDEBAR_MIN_WIDTH;
    use super::clamp_resized_panel_size;
    use super::next_dock_cycle_panel;

    #[test]
    fn next_dock_cycle_panel_works() {
        assert_eq!(next_dock_cycle_panel(&[true, true], None), Some(0));
        assert_eq!(next_dock_cycle_panel(&[true, true], Some(0)), Some(1));
        assert_eq!(next_dock_cycle_panel(&[true, true], Some(1)), None);
        assert_eq!(next_dock_cycle_panel(&[true], Some(0)), None);
        assert_eq!(next_dock_cycle_panel(&[false, true, false, true], None), Some(1));
        assert_eq!(next_dock_cycle_panel(&[true, false, true], Some(0)), Some(2));
        assert_eq!(next_dock_cycle_panel(&[true, true, false], Some(1)), None);
        assert_eq!(next_dock_cycle_panel(&[false], None), None);
        assert_eq!(next_dock_cycle_panel(&[], None), None);
    }

    #[test]
    fn clamp_resized_panel_size_works() {
        assert_eq!(clamp_resized_panel_size(px(300.4), Some(SIDEBAR_MIN_WIDTH)), px(300.));
        assert_eq!(clamp_resized_panel_size(px(100.), Some(SIDEBAR_MIN_WIDTH)), SIDEBAR_MIN_WIDTH);
        assert_eq!(clamp_resized_panel_size(px(100.), None), px(100.));
        assert_eq!(clamp_resized_panel_size(px(1.), None), RESIZE_HANDLE_SIZE);
    }
}
