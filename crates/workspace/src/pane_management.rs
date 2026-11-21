use std::cmp;
use std::collections::hash_map;
use std::sync::Arc;

use gpui::Action as _;
use gpui::App;
use gpui::AppContext as _;
use gpui::Context;
use gpui::Entity;
use gpui::EntityId;
use gpui::Focusable as _;
use ui::Window;

use super::ActivatePane;
use super::Event;
use super::MoveItemToPane;
use super::MoveItemToPaneInDirection;
use super::NewFile;
use super::Pane;
use super::SplitMode;
use super::Workspace;
use super::WorkspaceMode;
use super::pane;
use crate::dock::Dock;
use crate::item::ItemHandle;
use crate::mode::is_dragged_terminal;
use crate::pane_group::Member;
use crate::pane_group::PaneAxis;
use crate::pane_group::SplitDirection;

impl Workspace {
    pub(super) fn add_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Pane> {
        let pane = cx.new(|cx| {
            let mut pane = Pane::new(
                self.weak_handle(),
                self.project.clone(),
                Arc::clone(&self.pane_history_timestamp),
                Some(Arc::new(|dragged, _, cx| !is_dragged_terminal(dragged, cx))),
                NewFile.boxed_clone(),
                window,
                cx,
            );
            pane.set_can_split(Some(Arc::new(|_, _, _, _| true)));
            pane::set_editor_tab_bar_buttons(&mut pane, cx);
            pane.set_nav_history(self.nav_history.clone());
            pane
        });
        cx.subscribe_in(&pane, window, Self::handle_pane_event).detach();
        self.panes.push(pane.clone());

        window.focus(&pane.focus_handle(cx), cx);

        cx.emit(Event::PaneAdded(pane.clone()));
        pane
    }

    pub fn activate_item(
        &mut self,
        item: &dyn ItemHandle,
        activate_pane: bool,
        focus_item: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let result = self.panes.iter().find_map(|pane| pane.read(cx).index_for_item(item).map(|ix| (pane.clone(), ix)));
        if let Some((pane, ix)) = result {
            pane.update(cx, |pane, cx| pane.activate_item(ix, activate_pane, focus_item, window, cx));
            true
        } else {
            false
        }
    }

    pub(super) fn activate_pane_at_index(
        &mut self,
        action: &ActivatePane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panes = self.center.panes();
        if let Some(pane) = panes.get(action.0).map(|p| (*p).clone()) {
            window.focus(&pane.focus_handle(cx), cx);
        } else {
            self.split_and_move(self.active_pane.clone(), SplitDirection::Right, window, cx);
        }
    }

    pub(super) fn move_item_to_pane_at_index(
        &mut self,
        action: &MoveItemToPane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panes = self.center.panes();
        let destination = if let Some(&destination) = panes.get(action.destination) {
            destination.clone()
        } else {
            if self.active_pane.read(cx).items_len() < 2 {
                return;
            }
            let direction = SplitDirection::Right;
            let split_off_pane = self.find_pane_in_direction(direction, cx).unwrap_or_else(|| self.active_pane.clone());
            let new_pane = self.add_pane(window, cx);
            self.center.split(&split_off_pane, &new_pane, direction, cx);
            new_pane
        };

        move_active_item(&self.active_pane, &destination, action.focus, true, window, cx);
    }

    pub fn activate_next_pane(&mut self, window: &mut Window, cx: &mut App) {
        let panes = self.center.panes();
        if let Some(ix) = panes.iter().position(|pane| **pane == self.active_pane) {
            let next_ix = (ix + 1) % panes.len();
            let next_pane = panes[next_ix].clone();
            window.focus(&next_pane.focus_handle(cx), cx);
        }
    }

    pub fn activate_previous_pane(&mut self, window: &mut Window, cx: &mut App) {
        let panes = self.center.panes();
        if let Some(ix) = panes.iter().position(|pane| **pane == self.active_pane) {
            let prev_ix = cmp::min(ix.wrapping_sub(1), panes.len() - 1);
            let prev_pane = panes[prev_ix].clone();
            window.focus(&prev_pane.focus_handle(cx), cx);
        }
    }

    pub fn activate_last_pane(&mut self, window: &mut Window, cx: &mut App) {
        let last_pane = self.center.last_pane();
        window.focus(&last_pane.focus_handle(cx), cx);
    }

    pub fn activate_pane_in_direction(&mut self, direction: SplitDirection, window: &mut Window, cx: &mut App) {
        use ActivateInDirectionTarget as Target;
        enum Origin {
            LeftDock,
            BottomDock,
            Center,
        }

        let origin: Origin = [(&self.left_dock, Origin::LeftDock), (&self.bottom_dock, Origin::BottomDock)]
            .into_iter()
            .find_map(|(dock, origin)| {
                (dock.focus_handle(cx).contains_focused(window, cx) && dock.read(cx).is_open()).then_some(origin)
            })
            .unwrap_or(Origin::Center);

        let get_last_active_pane = || {
            let pane = self
                .last_active_center_pane
                .clone()
                .unwrap_or_else(|| self.panes.first().expect("There must be an active pane").downgrade())
                .upgrade()?;
            (pane.read(cx).items_len() != 0).then_some(pane)
        };

        let try_dock = |dock: &Entity<Dock>| dock.read(cx).is_open().then(|| Target::Dock(dock.clone()));

        let target = match (origin, direction) {
            // We're in the center, so we first try to go to a different pane,
            // otherwise try to go to a dock.
            (Origin::Center, direction) => {
                if let Some(pane) = self.find_pane_in_direction(direction, cx) {
                    Some(Target::Pane(pane))
                } else {
                    match direction {
                        SplitDirection::Up => None,
                        SplitDirection::Down => try_dock(&self.bottom_dock),
                        SplitDirection::Left => try_dock(&self.left_dock),
                        SplitDirection::Right => None,
                    }
                }
            }

            (Origin::LeftDock, SplitDirection::Right) => {
                if let Some(last_active_pane) = get_last_active_pane() {
                    Some(Target::Pane(last_active_pane))
                } else {
                    try_dock(&self.bottom_dock)
                }
            }

            (Origin::LeftDock, SplitDirection::Down) => try_dock(&self.bottom_dock),

            (Origin::BottomDock, SplitDirection::Up) => get_last_active_pane().map(Target::Pane),
            (Origin::BottomDock, SplitDirection::Left) => try_dock(&self.left_dock),

            _ => None,
        };

        match target {
            Some(ActivateInDirectionTarget::Pane(pane)) => {
                let pane = pane.read(cx);
                if let Some(item) = pane.active_item() {
                    item.item_focus_handle(cx).focus(window, cx);
                } else {
                    log::error!(
                        "Could not find a focus target when in switching focus in {direction} direction for a pane",
                    );
                }
            }
            Some(ActivateInDirectionTarget::Dock(dock)) => {
                // Defer this to avoid a panic when the dock's active panel is already on the stack.
                window.defer(cx, move |window, cx| {
                    let dock = dock.read(cx);
                    if let Some(panel) = dock.active_panel() {
                        panel.activation_focus_handle(cx).focus(window, cx);
                    } else {
                        log::error!("Could not find a focus target when in switching focus in {direction} direction for a {:?} dock", dock.position());
                    }
                });
            }
            None => {}
        }
    }

    pub fn move_item_to_pane_in_direction(
        &mut self,
        action: &MoveItemToPaneInDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let destination = if let Some(destination) = self.find_pane_in_direction(action.direction, cx) {
            destination
        } else {
            if self.active_pane.read(cx).items_len() < 2 {
                return;
            }
            let new_pane = self.add_pane(window, cx);
            self.center.split(&self.active_pane, &new_pane, action.direction, cx);
            new_pane
        };

        move_active_item(&self.active_pane, &destination, action.focus, true, window, cx);
    }

    pub fn find_pane_in_direction(&mut self, direction: SplitDirection, cx: &App) -> Option<Entity<Pane>> {
        self.center.find_pane_in_direction(&self.active_pane, direction, cx).cloned()
    }

    pub fn swap_pane_in_direction(&mut self, direction: SplitDirection, cx: &mut Context<Self>) {
        if let Some(to) = self.find_pane_in_direction(direction, cx) {
            self.center.swap(&self.active_pane, &to, cx);
            self.notify_mode(WorkspaceMode::Editor, cx);
        }
    }

    pub fn move_pane_to_border(&mut self, direction: SplitDirection, cx: &mut Context<Self>) {
        if self.center.move_to_border(&self.active_pane, direction, cx).expect("active pane is in the center group") {
            self.notify_mode(WorkspaceMode::Editor, cx);
        }
    }

    pub fn reset_pane_sizes(&mut self, cx: &mut Context<Self>) {
        self.center.reset_pane_sizes(cx);
        self.notify_mode(WorkspaceMode::Editor, cx);
    }

    fn handle_pane_focused(&mut self, pane: Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        // Terminal panel panes come through here too, but aren't content panes.
        if self.panes.contains(&pane) {
            self.content_pane_focused(&pane, window, cx);
        }
        self.switch_mode(WorkspaceMode::Editor, cx);
        self.flush_deferred_saves(window, cx);

        // This is explicitly hoisted out of the following check for pane identity as
        // terminal panel panes are not registered as a center panes.
        self.status_bar.update(cx, |status_bar, cx| {
            status_bar.set_active_pane(&pane, window, cx);
        });
        if self.active_pane != pane {
            self.set_active_pane(&pane, window, cx);
        }

        if self.last_active_center_pane.is_none() {
            self.last_active_center_pane = Some(pane.downgrade());
        }

        // If this pane is in a dock, preserve that dock when dismissing zoomed items.
        // This prevents the dock from closing when focus events fire during window activation.
        // We also preserve any dock whose active panel itself has focus — this covers
        // panels that don't implement `pane()` but can still be zoomed.
        let dock_to_preserve = self.all_docks().iter().find_map(|dock| {
            let dock_read = dock.read(cx);
            if let Some(panel) = dock_read.active_panel()
                && (panel.pane(cx).is_some_and(|dock_pane| dock_pane == pane)
                    || panel.panel_focus_handle(cx).contains_focused(window, cx))
            {
                return Some(dock_read.position());
            }
            None
        });

        if let Some(maximized) = &self.maximized_pane {
            let is_center_pane = self.panes.contains(&pane);
            if is_center_pane && maximized.upgrade().as_ref() != Some(&pane) {
                self.maximized_pane = None;
            }
        }

        self.dismiss_zoomed_items_to_reveal(dock_to_preserve, window, cx);
        if pane.read(cx).is_zoomed() {
            self.zoomed = Some(pane.downgrade().into());
        } else {
            self.zoomed = None;
        }
        self.zoomed_position = None;
        cx.emit(Event::ZoomChanged);

        pane.update(cx, |pane, _| {
            pane.track_alternate_file_items();
        });

        cx.notify();
    }

    pub(super) fn set_active_pane(&mut self, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        self.active_pane = pane.clone();
        self.active_item_path_changed(true, window, cx);
        self.last_active_center_pane = Some(pane.downgrade());
    }

    pub(super) fn handle_pane_event(
        &mut self,
        pane: &Entity<Pane>,
        event: &pane::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut serialize_workspace = true;
        match event {
            pane::Event::AddItem { item } => {
                item.added_to_pane(self, pane.clone(), window, cx);
                cx.emit(Event::ItemAdded { item: item.boxed_clone() });
            }
            pane::Event::Split { direction, mode } => match mode {
                SplitMode::EmptyPane => {
                    self.split_pane(pane.clone(), *direction, window, cx);
                }
                SplitMode::MovePane => {
                    self.split_and_move(pane.clone(), *direction, window, cx);
                }
            },
            pane::Event::JoinIntoNext => {
                self.join_pane_into_next(pane.clone(), window, cx);
            }
            pane::Event::JoinAll => {
                self.join_all_panes(window, cx);
            }
            pane::Event::Remove { focus_on_pane } => {
                self.remove_pane(pane.clone(), focus_on_pane.clone(), window, cx);
            }
            pane::Event::ActivateItem { local, focus_changed } => {
                window.invalidate_character_coordinates();
                // Center panes aren't rendered in Git History mode, so focusing one never
                // reaches `handle_pane_focused`; show them when an item asks for focus.
                if *focus_changed {
                    self.switch_mode(WorkspaceMode::Editor, cx);
                }

                pane.update(cx, |pane, _| {
                    pane.track_alternate_file_items();
                });
                serialize_workspace = *focus_changed || pane != self.active_pane();
                if pane == self.active_pane() {
                    self.active_item_path_changed(*focus_changed, window, cx);
                } else if *local {
                    self.set_active_pane(pane, window, cx);
                }
            }
            pane::Event::UserSavedItem { item, save_intent } => {
                cx.emit(Event::UserSavedItem {
                    pane: pane.downgrade(),
                    item: item.boxed_clone(),
                    save_intent: *save_intent,
                });
                serialize_workspace = false;
            }
            pane::Event::ChangeItemTitle => {
                if *pane == self.active_pane {
                    self.active_item_path_changed(false, window, cx);
                    self.notify_mode(WorkspaceMode::Editor, cx);
                }
                serialize_workspace = false;
            }
            pane::Event::RemovedItem { item } => {
                if item.project_path(cx).is_some_and(|path| self.is_deleted_history_path(&path, cx)) {
                    pane.update(cx, |pane, _| pane.nav_history_mut().remove_item(item.item_id()));
                }
                cx.emit(Event::ActiveItemChanged);
                self.update_window_edited(window, cx);
                if let hash_map::Entry::Occupied(entry) = self.panes_by_item.entry(item.item_id())
                    && entry.get().entity_id() == pane.entity_id()
                {
                    entry.remove();
                }
                cx.emit(Event::ItemRemoved { item_id: item.item_id() });
            }
            pane::Event::Focus => {
                window.invalidate_character_coordinates();
                self.handle_pane_focused(pane.clone(), window, cx);
            }
            pane::Event::ZoomIn => {
                if *pane == self.active_pane {
                    self.maximized_pane = None;
                    pane.update(cx, |pane, cx| pane.set_zoomed(true, cx));
                    if pane.read(cx).has_focus(window, cx) {
                        self.zoomed = Some(pane.downgrade().into());
                        self.zoomed_position = None;
                        cx.emit(Event::ZoomChanged);
                    }
                    self.notify_mode(WorkspaceMode::Editor, cx);
                }
            }
            pane::Event::ZoomOut => {
                pane.update(cx, |pane, cx| pane.set_zoomed(false, cx));
                if self.zoomed_position.is_none() {
                    self.zoomed = None;
                    cx.emit(Event::ZoomChanged);
                }
                self.notify_mode(WorkspaceMode::Editor, cx);
            }
            pane::Event::ItemPinned | pane::Event::ItemUnpinned => {}
        }

        if serialize_workspace {
            self.serialize_workspace(window, cx);
        }
    }

    pub fn split_pane(
        &mut self,
        pane_to_split: Entity<Pane>,
        split_direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        // History panes split within their own group, so a split started there
        // (drag to split, open to the side) stays in Git History mode. Like an
        // editor split, the new pane is neither activated nor focused: the
        // caller decides where focus goes, and the focused pane becomes active.
        if self.git_history_center.root.contains_pane(&pane_to_split) {
            self.zoom_git_history_pane(None, cx);
            let new_pane = self.add_git_history_pane(window, cx);
            self.git_history_center.split(&pane_to_split, &new_pane, split_direction, cx);
            self.notify_mode(WorkspaceMode::GitHistory, cx);
            return new_pane;
        }
        if self.terminal_center.root.contains_pane(&pane_to_split) {
            return self.split_terminal_pane_for_drop(&pane_to_split, split_direction, window, cx);
        }
        let new_pane = self.add_pane(window, cx);
        self.center.split(&pane_to_split, &new_pane, split_direction, cx);
        self.notify_mode(WorkspaceMode::Editor, cx);
        new_pane
    }

    pub fn split_and_move(
        &mut self,
        pane: Entity<Pane>,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A lone tab can't leave its pane: an empty pane opens on the other side
        // instead, which leaves the tab where it was asked to go, still focused.
        if pane.read(cx).items_len() < 2 {
            self.split_pane(pane.clone(), direction.opposite(), window, cx);
            window.focus(&pane.focus_handle(cx), cx);
            return;
        }
        let Some(item) = pane.update(cx, |pane, cx| pane.take_active_item(window, cx)) else {
            return;
        };
        let new_pane = self.add_pane(window, cx);
        new_pane.update(cx, |pane, cx| pane.add_item(item, true, true, None, window, cx));
        self.center.split(&pane, &new_pane, direction, cx);
        self.notify_mode(WorkspaceMode::Editor, cx);
    }

    pub fn join_all_panes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active_item = self.active_pane.read(cx).active_item();
        for pane in &self.panes {
            join_pane_into_active(&self.active_pane, pane, window, cx);
        }
        if let Some(active_item) = active_item {
            self.activate_item(active_item.as_ref(), true, true, window, cx);
        }
        self.notify_mode(WorkspaceMode::Editor, cx);
    }

    pub fn join_pane_into_next(&mut self, pane: Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        let next_pane = self
            .find_pane_in_direction(SplitDirection::Right, cx)
            .or_else(|| self.find_pane_in_direction(SplitDirection::Down, cx))
            .or_else(|| self.find_pane_in_direction(SplitDirection::Left, cx))
            .or_else(|| self.find_pane_in_direction(SplitDirection::Up, cx));
        let Some(next_pane) = next_pane else {
            return;
        };
        move_all_items(&pane, &next_pane, window, cx);
        self.notify_mode(WorkspaceMode::Editor, cx);
    }

    fn remove_pane(
        &mut self,
        pane: Entity<Pane>,
        focus_on: Option<Entity<Pane>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.center.remove(&pane, cx).expect("pane is in the center group") {
            if self.maximized_pane.as_ref().and_then(gpui::WeakEntity::upgrade).as_ref() == Some(&pane) {
                self.maximized_pane = None;
            }
            self.force_remove_pane(&pane, focus_on.as_ref(), window, cx);

            for removed_item in pane.read(cx).items() {
                self.panes_by_item.remove(&removed_item.item_id());
            }

            cx.notify();
        } else {
            self.active_item_path_changed(true, window, cx);
        }
        cx.emit(Event::PaneRemoved);
    }

    pub fn panes_mut(&mut self) -> &mut [Entity<Pane>] {
        &mut self.panes
    }

    pub fn panes(&self) -> &[Entity<Pane>] {
        &self.panes
    }

    pub const fn active_pane(&self) -> &Entity<Pane> {
        &self.active_pane
    }

    pub fn focused_pane(&self, window: &Window, cx: &App) -> Entity<Pane> {
        for dock in self.all_docks() {
            if dock.focus_handle(cx).contains_focused(window, cx)
                && let Some(pane) = dock.read(cx).active_panel().and_then(|panel| panel.pane(cx))
            {
                return pane;
            }
        }
        self.active_pane().clone()
    }

    pub fn adjacent_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Pane> {
        self.adjacent_pane_of(&self.active_pane.clone(), window, cx)
    }

    pub fn adjacent_pane_of(
        &mut self,
        origin: &Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        // Search the group holding `origin`: the other one isn't painted in
        // this mode, so its layout is stale.
        let pane_group = if self.git_history_center.root.contains_pane(origin) {
            &mut self.git_history_center
        } else if self.terminal_center.root.contains_pane(origin) {
            &mut self.terminal_center
        } else {
            &mut self.center
        };
        pane_group
            .find_pane_in_direction(origin, SplitDirection::Right, cx)
            .cloned()
            .unwrap_or_else(|| self.split_pane(origin.clone(), SplitDirection::Right, window, cx))
    }

    pub fn pane_for(&self, handle: &dyn ItemHandle) -> Option<Entity<Pane>> {
        self.pane_for_item_id(handle.item_id())
    }

    pub fn pane_for_item_id(&self, item_id: EntityId) -> Option<Entity<Pane>> {
        let weak_pane = self.panes_by_item.get(&item_id)?;
        weak_pane.upgrade()
    }

    pub(super) fn remove_panes(&mut self, member: Member, window: &mut Window, cx: &mut Context<Workspace>) {
        match member {
            Member::Axis(PaneAxis { members, .. }) => {
                for child in &members {
                    self.remove_panes(child.clone(), window, cx);
                }
            }
            Member::Pane(pane) => {
                self.force_remove_pane(&pane, None, window, cx);
            }
        }
    }

    pub(super) fn force_remove_pane(
        &mut self,
        pane: &Entity<Pane>,
        focus_on: Option<&Entity<Pane>>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let removing_active_pane = self.active_pane() == pane;
        self.panes.retain(|p| p != pane);
        if let Some(focus_on) = focus_on {
            if removing_active_pane {
                self.set_active_pane(focus_on, window, cx);
            }
            focus_on.update(cx, |pane, cx| window.focus(&pane.focus_handle(cx), cx));
        } else if removing_active_pane {
            let fallback_pane = self.panes.last().expect("workspace always has a pane").clone();
            self.set_active_pane(&fallback_pane, window, cx);
            if !self.has_active_modal(window, cx) {
                fallback_pane.update(cx, |pane, cx| window.focus(&pane.focus_handle(cx), cx));
            }
        }
        if self.last_active_center_pane == Some(pane.downgrade()) {
            self.last_active_center_pane = None;
        }
        cx.notify();
    }
}

#[derive(Clone)]
enum ActivateInDirectionTarget {
    Pane(Entity<Pane>),
    Dock(Entity<Dock>),
}

fn join_pane_into_active(active_pane: &Entity<Pane>, pane: &Entity<Pane>, window: &mut Window, cx: &mut App) {
    if pane == active_pane {
    } else if pane.read(cx).items_len() == 0 {
        pane.update(cx, |_, cx| {
            cx.emit(pane::Event::Remove { focus_on_pane: None });
        });
    } else {
        move_all_items(pane, active_pane, window, cx);
    }
}

fn move_all_items(from_pane: &Entity<Pane>, to_pane: &Entity<Pane>, window: &mut Window, cx: &mut App) {
    let destination_is_different = from_pane != to_pane;
    let mut moved_items = 0;
    for (item_ix, item_handle) in
        from_pane.read(cx).items().enumerate().map(|(ix, item)| (ix, item.clone())).collect::<Vec<_>>()
    {
        let ix = item_ix - moved_items;
        if destination_is_different {
            // Close item from previous pane
            from_pane.update(cx, |source, cx| {
                source.remove_item_and_focus_on_pane(ix, false, to_pane.clone(), window, cx);
            });
            moved_items += 1;
        }

        // This automatically removes duplicate items in the pane
        to_pane.update(cx, |destination, cx| {
            destination.add_item(item_handle, true, true, None, window, cx);
            window.focus(&destination.focus_handle(cx), cx);
        });
    }
}

pub fn move_item(
    source: &Entity<Pane>,
    destination: &Entity<Pane>,
    item_id_to_move: EntityId,
    destination_index: usize,
    activate: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((item_ix, item_handle)) = source
        .read(cx)
        .items()
        .enumerate()
        .find(|(_, item_handle)| item_handle.item_id() == item_id_to_move)
        .map(|(ix, item)| (ix, item.clone()))
    else {
        // Tab was closed during drag
        return;
    };

    if source != destination {
        // Close item from previous pane
        source.update(cx, |source, cx| {
            source.remove_item_and_focus_on_pane(item_ix, false, destination.clone(), window, cx);
        });
    }

    // This automatically removes duplicate items in the pane
    destination.update(cx, |destination, cx| {
        destination.add_item_inner(item_handle, activate, activate, activate, Some(destination_index), window, cx);
        if activate {
            window.focus(&destination.focus_handle(cx), cx);
        }
    });
}

pub fn move_active_item(
    source: &Entity<Pane>,
    destination: &Entity<Pane>,
    focus_destination: bool,
    close_if_empty: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if source == destination {
        return;
    }
    let Some(active_item) = source.read(cx).active_item() else {
        return;
    };
    source.update(cx, |source_pane, cx| {
        let item_id = active_item.item_id();
        source_pane.remove_item(item_id, false, close_if_empty, window, cx);
        destination.update(cx, |target_pane, cx| {
            target_pane.add_item(
                active_item,
                focus_destination,
                focus_destination,
                Some(target_pane.items_len()),
                window,
                cx,
            );
        });
    });
}
