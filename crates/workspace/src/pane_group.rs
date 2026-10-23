use std::sync::Arc;

use anyhow::Result;
use gpui::Along;
use gpui::AnyView;
use gpui::AnyWeakView;
use gpui::Axis;
use gpui::Bounds;
use gpui::Entity;
use gpui::Hsla;
use gpui::IntoElement;
use gpui::Pixels;
use gpui::Point;
use gpui::StyleRefinement;
use gpui::WeakEntity;
use gpui::Window;
use gpui::point;
use gpui::size;
use parking_lot::Mutex;
use project::Project;
use schemars::JsonSchema;
use serde::Deserialize;
use settings::Settings;
use ui::prelude::*;

use crate::AppState;
use crate::Pane;
use crate::Workspace;
use crate::WorkspaceSettings;
use crate::pane_group::element::pane_axis;
use crate::workspace_settings::PaneSplitDirectionHorizontal;
use crate::workspace_settings::PaneSplitDirectionVertical;

pub const HANDLE_HITBOX_SIZE: f32 = 4.0;
const HORIZONTAL_MIN_SIZE: f32 = 80.;
const VERTICAL_MIN_SIZE: f32 = 100.;

/// One or many panes, arranged in a horizontal or vertical axis due to a split.
/// Panes have all their tabs and capabilities preserved, and can be split again or resized.
/// Single-pane group is a regular pane.
#[derive(Clone)]
pub struct PaneGroup {
    pub root: Member,
    pub is_center: bool,
}

pub struct PaneRenderResult {
    pub element: gpui::AnyElement,
    pub contains_active_pane: bool,
}

impl PaneGroup {
    pub const fn with_root(root: Member) -> Self {
        Self { root, is_center: false }
    }

    pub const fn new(pane: Entity<Pane>) -> Self {
        Self { root: Member::Pane(pane), is_center: false }
    }

    pub const fn set_is_center(&mut self, is_center: bool) {
        self.is_center = is_center;
    }

    pub fn split(&mut self, old_pane: &Entity<Pane>, new_pane: &Entity<Pane>, direction: SplitDirection, cx: &mut App) {
        let found = match &mut self.root {
            Member::Pane(pane) => {
                if pane == old_pane {
                    self.root = Member::new_axis(old_pane.clone(), new_pane.clone(), direction);
                    true
                } else {
                    false
                }
            }
            Member::Axis(axis) => axis.split(old_pane, new_pane, direction),
        };

        // If the pane wasn't found, fall back to splitting the first pane in the tree.
        if !found {
            let first_pane = self.root.first_pane();
            match &mut self.root {
                Member::Pane(_) => {
                    self.root = Member::new_axis(first_pane, new_pane.clone(), direction);
                }
                Member::Axis(axis) => {
                    let _ = axis.split(&first_pane, new_pane, direction);
                }
            }
        }

        self.mark_positions(cx);
    }

    pub fn bounding_box_for_pane(&self, pane: &Entity<Pane>) -> Option<Bounds<Pixels>> {
        match &self.root {
            Member::Pane(_) => None,
            Member::Axis(axis) => axis.bounding_box_for_pane(pane),
        }
    }

    pub fn full_height_column_count(&self) -> usize {
        self.root.full_height_column_count()
    }

    pub fn pane_at_pixel_position(&self, coordinate: Point<Pixels>) -> Option<&Entity<Pane>> {
        match &self.root {
            Member::Pane(pane) => Some(pane),
            Member::Axis(axis) => axis.pane_at_pixel_position(coordinate),
        }
    }

    /// Moves active pane to span the entire border in the given direction,
    /// similar to Vim ctrl+w shift-[hjkl] motion.
    ///
    /// Returns:
    /// - Ok(true) if it found and moved a pane
    /// - Ok(false) if it found but did not move the pane
    /// - Err(_) if it did not find the pane
    pub fn move_to_border(
        &mut self,
        active_pane: &Entity<Pane>,
        direction: SplitDirection,
        cx: &mut App,
    ) -> Result<bool> {
        if let Some(pane) = self.find_pane_at_border(direction)
            && pane == active_pane
        {
            return Ok(false);
        }

        if !self.remove_internal(active_pane)? {
            return Ok(false);
        }

        if let Member::Axis(root) = &mut self.root
            && direction.axis() == root.axis
        {
            let idx = if direction.increasing() { root.members.len() } else { 0 };
            root.insert_pane(idx, active_pane);
            self.mark_positions(cx);
            return Ok(true);
        }

        let members = if direction.increasing() {
            vec![self.root.clone(), Member::Pane(active_pane.clone())]
        } else {
            vec![Member::Pane(active_pane.clone()), self.root.clone()]
        };
        self.root = Member::Axis(PaneAxis::new(direction.axis(), members));
        self.mark_positions(cx);
        Ok(true)
    }

    fn find_pane_at_border(&self, direction: SplitDirection) -> Option<&Entity<Pane>> {
        match &self.root {
            Member::Pane(pane) => Some(pane),
            Member::Axis(axis) => axis.find_pane_at_border(direction),
        }
    }

    /// Returns:
    /// - Ok(true) if it found and removed a pane
    /// - Ok(false) if it found but did not remove the pane
    /// - Err(_) if it did not find the pane
    pub fn remove(&mut self, pane: &Entity<Pane>, cx: &mut App) -> Result<bool> {
        let result = self.remove_internal(pane);
        if let Ok(true) = result {
            self.mark_positions(cx);
        }
        result
    }

    fn remove_internal(&mut self, pane: &Entity<Pane>) -> Result<bool> {
        match &mut self.root {
            Member::Pane(_) => Ok(false),
            Member::Axis(axis) => {
                if let Some(last_pane) = axis.remove(pane)? {
                    self.root = last_pane;
                }
                Ok(true)
            }
        }
    }

    pub fn resize(
        &mut self,
        pane: &Entity<Pane>,
        direction: Axis,
        amount: Pixels,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) {
        match &mut self.root {
            Member::Pane(_) => {}
            Member::Axis(axis) => {
                let _ = axis.resize(pane, direction, amount, bounds);
            }
        };
        self.mark_positions(cx);
    }

    pub fn reset_pane_sizes(&mut self, cx: &mut App) {
        match &mut self.root {
            Member::Pane(_) => {}
            Member::Axis(axis) => {
                axis.reset_pane_sizes();
            }
        };
        self.mark_positions(cx);
    }

    pub fn swap(&mut self, from: &Entity<Pane>, to: &Entity<Pane>, cx: &mut App) {
        match &mut self.root {
            Member::Pane(_) => {}
            Member::Axis(axis) => axis.swap(from, to),
        };
        self.mark_positions(cx);
    }

    pub fn mark_positions(&mut self, cx: &mut App) {
        self.root.mark_positions(self.is_center, cx);
    }

    pub fn render(
        &self,
        zoomed: Option<&AnyWeakView>,
        maximized: Option<&WeakEntity<Pane>>,
        render_cx: &dyn PaneLeaderDecorator,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        self.root.render(0, zoomed, maximized, render_cx, window, cx).element
    }

    pub fn panes(&self) -> Vec<&Entity<Pane>> {
        let mut panes = Vec::new();
        self.root.collect_panes(&mut panes);
        panes
    }

    pub fn first_pane(&self) -> Entity<Pane> {
        self.root.first_pane()
    }

    pub fn last_pane(&self) -> Entity<Pane> {
        self.root.last_pane()
    }

    pub fn find_pane_in_direction(
        &mut self,
        active_pane: &Entity<Pane>,
        direction: SplitDirection,
        cx: &App,
    ) -> Option<&Entity<Pane>> {
        let bounding_box = self.bounding_box_for_pane(active_pane)?;
        let cursor = active_pane.read(cx).pixel_position_of_cursor(cx);
        let center = match cursor {
            Some(cursor) if bounding_box.contains(&cursor) => cursor,
            _ => bounding_box.center(),
        };

        let distance_to_next = crate::HANDLE_HITBOX_SIZE;

        let target = match direction {
            SplitDirection::Left => Point::new(bounding_box.left() - distance_to_next.into(), center.y),
            SplitDirection::Right => Point::new(bounding_box.right() + distance_to_next.into(), center.y),
            SplitDirection::Up => Point::new(center.x, bounding_box.top() - distance_to_next.into()),
            SplitDirection::Down => Point::new(center.x, bounding_box.bottom() + distance_to_next.into()),
        };
        self.pane_at_pixel_position(target)
    }

    pub fn invert_axies(&mut self, cx: &mut App) {
        self.root.invert_pane_axies();
        self.mark_positions(cx);
    }
}

#[derive(Debug, Clone)]
pub enum Member {
    Axis(PaneAxis),
    Pane(Entity<Pane>),
}

impl Member {
    pub fn mark_positions(&mut self, in_center_group: bool, cx: &mut App) {
        match self {
            Member::Axis(pane_axis) => {
                for member in &mut pane_axis.members {
                    member.mark_positions(in_center_group, cx);
                }
            }
            Member::Pane(entity) => entity.update(cx, |pane, _| {
                pane.in_center_group = in_center_group;
            }),
        }
    }

    fn full_height_column_count(&self) -> usize {
        match self {
            Member::Pane(_) => 1,
            Member::Axis(axis) => axis.full_height_column_count(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct PaneRenderContext<'a> {
    pub project: &'a Entity<Project>,

    pub active_pane: &'a Entity<Pane>,
    pub app_state: &'a Arc<AppState>,
    pub workspace: &'a WeakEntity<Workspace>,
}

#[derive(Default)]
pub struct LeaderDecoration {
    border: Option<Hsla>,
    status_box: Option<AnyElement>,
}

pub trait PaneLeaderDecorator {
    fn decorate(&self, pane: &Entity<Pane>, cx: &App) -> LeaderDecoration;
    fn active_pane(&self) -> &Entity<Pane>;
    fn workspace(&self) -> &WeakEntity<Workspace>;
}

pub struct ActivePaneDecorator<'a> {
    active_pane: &'a Entity<Pane>,
    workspace: &'a WeakEntity<Workspace>,
}

impl<'a> ActivePaneDecorator<'a> {
    pub const fn new(active_pane: &'a Entity<Pane>, workspace: &'a WeakEntity<Workspace>) -> Self {
        Self { active_pane, workspace }
    }
}

impl PaneLeaderDecorator for ActivePaneDecorator<'_> {
    fn decorate(&self, _: &Entity<Pane>, _: &App) -> LeaderDecoration {
        LeaderDecoration::default()
    }
    fn active_pane(&self) -> &Entity<Pane> {
        self.active_pane
    }

    fn workspace(&self) -> &WeakEntity<Workspace> {
        self.workspace
    }
}

impl PaneLeaderDecorator for PaneRenderContext<'_> {
    fn decorate(&self, _: &Entity<Pane>, _: &App) -> LeaderDecoration {
        LeaderDecoration::default()
    }

    fn active_pane(&self) -> &Entity<Pane> {
        self.active_pane
    }

    fn workspace(&self) -> &WeakEntity<Workspace> {
        self.workspace
    }
}

impl Member {
    fn new_axis(old_pane: Entity<Pane>, new_pane: Entity<Pane>, direction: SplitDirection) -> Self {
        use Axis::Horizontal;
        use Axis::Vertical;
        use SplitDirection::Down;
        use SplitDirection::Left;
        use SplitDirection::Right;
        use SplitDirection::Up;

        let axis = match direction {
            Up | Down => Vertical,
            Left | Right => Horizontal,
        };

        let members = match direction {
            Up | Left => vec![Member::Pane(new_pane), Member::Pane(old_pane)],
            Down | Right => vec![Member::Pane(old_pane), Member::Pane(new_pane)],
        };

        Member::Axis(PaneAxis::new(axis, members))
    }

    fn first_pane(&self) -> Entity<Pane> {
        match self {
            Member::Axis(axis) => axis.members[0].first_pane(),
            Member::Pane(pane) => pane.clone(),
        }
    }

    fn last_pane(&self) -> Entity<Pane> {
        match self {
            Member::Axis(axis) => axis.members.last().unwrap().last_pane(),
            Member::Pane(pane) => pane.clone(),
        }
    }

    pub fn render(
        &self,
        basis: usize,
        zoomed: Option<&AnyWeakView>,
        maximized: Option<&WeakEntity<Pane>>,
        render_cx: &dyn PaneLeaderDecorator,
        window: &mut Window,
        cx: &mut App,
    ) -> PaneRenderResult {
        match self {
            Member::Pane(pane) => {
                if zoomed == Some(&pane.downgrade().into()) {
                    return PaneRenderResult { element: div().into_any(), contains_active_pane: false };
                }

                let is_maximized = if let Some(maximized) = maximized {
                    if maximized.upgrade().as_ref() != Some(pane) {
                        return PaneRenderResult { element: div().into_any(), contains_active_pane: false };
                    }
                    true
                } else {
                    false
                };

                let decoration = render_cx.decorate(pane, cx);
                let is_active = pane == render_cx.active_pane();

                let pane = div()
                    .relative()
                    .size_full()
                    .when(is_maximized, |this| {
                        this.bg(cx.theme().colors().background)
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .shadow_lg()
                            .overflow_hidden()
                    })
                    .child(AnyView::from(pane.clone()).cached(StyleRefinement::default().v_flex().size_full()))
                    .when_some(decoration.border, |this, color| {
                        this.child(div().absolute().size_full().left_0().top_0().border_2().border_color(color))
                    })
                    .children(decoration.status_box);

                PaneRenderResult {
                    element: div()
                        .relative()
                        .flex_1()
                        .size_full()
                        .when(is_maximized, |this| this.p_2())
                        .child(pane)
                        .into_any(),
                    contains_active_pane: is_active,
                }
            }
            Member::Axis(axis) => axis.render(basis + 1, zoomed, maximized, render_cx, window, cx),
        }
    }

    pub fn contains_pane(&self, needle: &Entity<Pane>) -> bool {
        match self {
            Member::Pane(pane) => pane == needle,
            Member::Axis(axis) => axis.members.iter().any(|member| member.contains_pane(needle)),
        }
    }

    fn collect_panes<'a>(&'a self, panes: &mut Vec<&'a Entity<Pane>>) {
        match self {
            Member::Axis(axis) => {
                for member in &axis.members {
                    member.collect_panes(panes);
                }
            }
            Member::Pane(pane) => panes.push(pane),
        }
    }

    fn invert_pane_axies(&mut self) {
        match self {
            Self::Axis(axis) => {
                axis.axis = axis.axis.invert();
                for member in &mut axis.members {
                    member.invert_pane_axies();
                }
            }
            Self::Pane(_) => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaneAxis {
    pub axis: Axis,
    pub members: Vec<Member>,
    pub flexes: Arc<Mutex<Vec<f32>>>,
    pub bounding_boxes: Arc<Mutex<Vec<Option<Bounds<Pixels>>>>>,
}

impl PaneAxis {
    pub fn new(axis: Axis, members: Vec<Member>) -> Self {
        let flexes = Arc::new(Mutex::new(vec![1.; members.len()]));
        let bounding_boxes = Arc::new(Mutex::new(vec![None; members.len()]));
        Self { axis, members, flexes, bounding_boxes }
    }

    pub fn load(axis: Axis, members: Vec<Member>, flexes: Option<Vec<f32>>) -> Self {
        let mut flexes = flexes.unwrap_or_else(|| vec![1.; members.len()]);
        if flexes.len() != members.len() || (flexes.iter().copied().sum::<f32>() - flexes.len() as f32).abs() >= 0.001 {
            flexes = vec![1.; members.len()];
        }

        let flexes = Arc::new(Mutex::new(flexes));
        let bounding_boxes = Arc::new(Mutex::new(vec![None; members.len()]));
        Self { axis, members, flexes, bounding_boxes }
    }

    fn split(&mut self, old_pane: &Entity<Pane>, new_pane: &Entity<Pane>, direction: SplitDirection) -> bool {
        for (mut idx, member) in self.members.iter_mut().enumerate() {
            match member {
                Member::Axis(axis) => {
                    if axis.split(old_pane, new_pane, direction) {
                        return true;
                    }
                }
                Member::Pane(pane) => {
                    if pane == old_pane {
                        if direction.axis() == self.axis {
                            if direction.increasing() {
                                idx += 1;
                            }
                            self.insert_pane(idx, new_pane);
                        } else {
                            *member = Member::new_axis(old_pane.clone(), new_pane.clone(), direction);
                        }
                        return true;
                    }
                }
            }
        }
        false
    }

    fn insert_pane(&mut self, idx: usize, new_pane: &Entity<Pane>) {
        self.members.insert(idx, Member::Pane(new_pane.clone()));
        *self.flexes.lock() = vec![1.; self.members.len()];
    }

    fn find_pane_at_border(&self, direction: SplitDirection) -> Option<&Entity<Pane>> {
        if self.axis != direction.axis() {
            return None;
        }
        let member = if direction.increasing() { self.members.last() } else { self.members.first() };
        member.and_then(|e| match e {
            Member::Pane(pane) => Some(pane),
            Member::Axis(_) => None,
        })
    }

    fn remove(&mut self, pane_to_remove: &Entity<Pane>) -> Result<Option<Member>> {
        let mut found_pane = false;
        let mut remove_member = None;
        for (idx, member) in self.members.iter_mut().enumerate() {
            match member {
                Member::Axis(axis) => {
                    if let Ok(last_pane) = axis.remove(pane_to_remove) {
                        if let Some(last_pane) = last_pane {
                            *member = last_pane;
                        }
                        found_pane = true;
                        break;
                    }
                }
                Member::Pane(pane) => {
                    if pane == pane_to_remove {
                        found_pane = true;
                        remove_member = Some(idx);
                        break;
                    }
                }
            }
        }

        if found_pane {
            if let Some(idx) = remove_member {
                self.members.remove(idx);
                *self.flexes.lock() = vec![1.; self.members.len()];
            }

            if self.members.len() == 1 {
                let result = self.members.pop();
                *self.flexes.lock() = vec![1.; self.members.len()];
                Ok(result)
            } else {
                Ok(None)
            }
        } else {
            anyhow::bail!("Pane not found");
        }
    }

    fn reset_pane_sizes(&self) {
        *self.flexes.lock() = vec![1.; self.members.len()];
        for member in &self.members {
            if let Member::Axis(axis) = member {
                axis.reset_pane_sizes();
            }
        }
    }

    fn resize(&mut self, pane: &Entity<Pane>, axis: Axis, amount: Pixels, bounds: &Bounds<Pixels>) -> Option<bool> {
        let container_size =
            self.bounding_boxes.lock().iter().filter_map(|e| *e).reduce(|acc, e| acc.union(&e)).unwrap_or(*bounds).size;

        let found_pane = self.members.iter().any(|member| matches!(member, Member::Pane(p) if p == pane));

        if found_pane && self.axis != axis {
            return Some(false); // pane found but this is not the correct axis direction
        }
        let mut found_axis_index: Option<usize> = None;
        if !found_pane {
            for (i, pa) in self.members.iter_mut().enumerate() {
                if let Member::Axis(pa) = pa
                    && let Some(done) = pa.resize(pane, axis, amount, bounds)
                {
                    if done {
                        return Some(true); // pane found and operations already done
                    } else if self.axis != axis {
                        return Some(false); // pane found but this is not the correct axis direction
                    } else {
                        found_axis_index = Some(i); // pane found and this is correct direction
                    }
                }
            }
            found_axis_index?; // no pane found
        }

        let min_size = match axis {
            Axis::Horizontal => px(HORIZONTAL_MIN_SIZE),
            Axis::Vertical => px(VERTICAL_MIN_SIZE),
        };
        let mut flexes = self.flexes.lock();

        let ix = if found_pane {
            self.members.iter().position(|m| if let Member::Pane(p) = m { p == pane } else { false })
        } else {
            found_axis_index
        };

        if ix.is_none() {
            return Some(true);
        }

        let ix = ix.unwrap_or(0);

        let size = move |ix, flexes: &[f32]| container_size.along(axis) * (flexes[ix] / flexes.len() as f32);

        // Don't allow resizing to less than the minimum size, if elements are already too small
        if min_size - px(1.) > size(ix, flexes.as_slice()) {
            return Some(true);
        }

        let flex_changes = |pixel_dx, target_ix: usize, next: isize, flexes: &[f32]| {
            let flex_change = flexes.len() as f32 * pixel_dx / container_size.along(axis);
            let current_target_flex = flexes[target_ix] + flex_change;
            let next_target_flex = flexes[(target_ix.cast_signed() + next) as usize] - flex_change;
            (current_target_flex, next_target_flex)
        };

        let apply_changes = |current_ix: usize, proposed_current_pixel_change: Pixels, flexes: &mut [f32]| {
            let next_target_size = Pixels::max(size(current_ix + 1, flexes) - proposed_current_pixel_change, min_size);
            let current_target_size =
                Pixels::max(size(current_ix, flexes) + size(current_ix + 1, flexes) - next_target_size, min_size);

            let current_pixel_change = current_target_size - size(current_ix, flexes);

            let (current_target_flex, next_target_flex) = flex_changes(current_pixel_change, current_ix, 1, flexes);

            flexes[current_ix] = current_target_flex;
            flexes[current_ix + 1] = next_target_flex;
        };

        if ix + 1 == flexes.len() {
            apply_changes(ix - 1, -1.0 * amount, flexes.as_mut_slice());
        } else {
            apply_changes(ix, amount, flexes.as_mut_slice());
        }
        Some(true)
    }

    fn swap(&mut self, from: &Entity<Pane>, to: &Entity<Pane>) {
        for member in &mut self.members {
            match member {
                Member::Axis(axis) => axis.swap(from, to),
                Member::Pane(pane) => {
                    if pane == from {
                        *member = Member::Pane(to.clone());
                    } else if pane == to {
                        *member = Member::Pane(from.clone())
                    }
                }
            }
        }
    }

    fn bounding_box_for_pane(&self, pane: &Entity<Pane>) -> Option<Bounds<Pixels>> {
        debug_assert!(self.members.len() == self.bounding_boxes.lock().len());

        for (idx, member) in self.members.iter().enumerate() {
            match member {
                Member::Pane(found) => {
                    if pane == found {
                        return self.bounding_boxes.lock()[idx];
                    }
                }
                Member::Axis(axis) => {
                    if let Some(rect) = axis.bounding_box_for_pane(pane) {
                        return Some(rect);
                    }
                }
            }
        }
        None
    }

    fn pane_at_pixel_position(&self, coordinate: Point<Pixels>) -> Option<&Entity<Pane>> {
        debug_assert!(self.members.len() == self.bounding_boxes.lock().len());

        let bounding_boxes = self.bounding_boxes.lock();

        for (idx, member) in self.members.iter().enumerate() {
            if let Some(coordinates) = bounding_boxes[idx]
                && coordinates.contains(&coordinate)
            {
                return match member {
                    Member::Pane(found) => Some(found),
                    Member::Axis(axis) => axis.pane_at_pixel_position(coordinate),
                };
            }
        }
        None
    }

    fn full_height_column_count(&self) -> usize {
        match self.axis {
            Axis::Horizontal => self.members.iter().map(Member::full_height_column_count).sum::<usize>().max(1),
            Axis::Vertical => self.members.iter().map(Member::full_height_column_count).max().unwrap_or(1),
        }
    }

    fn render(
        &self,
        basis: usize,
        zoomed: Option<&AnyWeakView>,
        maximized: Option<&WeakEntity<Pane>>,
        render_cx: &dyn PaneLeaderDecorator,
        window: &mut Window,
        cx: &mut App,
    ) -> PaneRenderResult {
        if let Some(maximized) = maximized
            && let Some(maximized_pane) = maximized.upgrade()
        {
            for member in &self.members {
                if member.contains_pane(&maximized_pane) {
                    return member.render(basis, zoomed, Some(maximized), render_cx, window, cx);
                }
            }
        }

        debug_assert!(self.members.len() == self.flexes.lock().len());
        let mut active_pane_ix = None;
        let mut contains_active_pane = false;
        let mut leaf_panes = vec![None; self.members.len()];

        let rendered_children = self
            .members
            .iter()
            .enumerate()
            .map(|(ix, member)| {
                match member {
                    Member::Pane(pane) => {
                        leaf_panes[ix] = Some(pane.clone());
                        if pane == render_cx.active_pane() {
                            active_pane_ix = pane.read(cx).has_focus(window, cx).then_some(ix);
                            contains_active_pane = true;
                        }
                    }
                    Member::Axis(_) => {}
                }

                let result = member.render((basis + ix) * 10, zoomed, None, render_cx, window, cx);
                if result.contains_active_pane {
                    contains_active_pane = true;
                }
                result.element.into_any_element()
            })
            .collect::<Vec<_>>();

        let element = pane_axis(
            self.axis,
            basis,
            self.flexes.clone(),
            self.bounding_boxes.clone(),
            render_cx.workspace().clone(),
        )
        .with_leaf_panes(leaf_panes)
        .children(rendered_children)
        .with_active_pane(active_pane_ix)
        .into_any_element();

        PaneRenderResult { element, contains_active_pane }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Up,
    Down,
    Left,
    Right,
}

impl std::fmt::Display for SplitDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitDirection::Up => write!(f, "up"),
            SplitDirection::Down => write!(f, "down"),
            SplitDirection::Left => write!(f, "left"),
            SplitDirection::Right => write!(f, "right"),
        }
    }
}

impl SplitDirection {
    pub const fn all() -> [Self; 4] {
        [Self::Up, Self::Down, Self::Left, Self::Right]
    }

    pub fn vertical(cx: &mut App) -> Self {
        match WorkspaceSettings::get_global(cx).pane_split_direction_vertical {
            PaneSplitDirectionVertical::Left => SplitDirection::Left,
            PaneSplitDirectionVertical::Right => SplitDirection::Right,
        }
    }

    pub fn horizontal(cx: &mut App) -> Self {
        match WorkspaceSettings::get_global(cx).pane_split_direction_horizontal {
            PaneSplitDirectionHorizontal::Down => SplitDirection::Down,
            PaneSplitDirectionHorizontal::Up => SplitDirection::Up,
        }
    }

    pub fn edge(&self, rect: Bounds<Pixels>) -> Pixels {
        match self {
            Self::Up => rect.origin.y,
            Self::Down => rect.bottom_left().y,
            Self::Left => rect.bottom_left().x,
            Self::Right => rect.bottom_right().x,
        }
    }

    pub fn along_edge(&self, bounds: Bounds<Pixels>, length: Pixels) -> Bounds<Pixels> {
        match self {
            Self::Up => Bounds { origin: bounds.origin, size: size(bounds.size.width, length) },
            Self::Down => Bounds {
                origin: point(bounds.bottom_left().x, bounds.bottom_left().y - length),
                size: size(bounds.size.width, length),
            },
            Self::Left => Bounds { origin: bounds.origin, size: size(length, bounds.size.height) },
            Self::Right => Bounds {
                origin: point(bounds.bottom_right().x - length, bounds.bottom_left().y),
                size: size(length, bounds.size.height),
            },
        }
    }

    pub const fn axis(&self) -> Axis {
        match self {
            Self::Up | Self::Down => Axis::Vertical,
            Self::Left | Self::Right => Axis::Horizontal,
        }
    }

    pub const fn increasing(&self) -> bool {
        match self {
            Self::Left | Self::Up => false,
            Self::Down | Self::Right => true,
        }
    }

    pub const fn opposite(&self) -> SplitDirection {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

mod element {
    use std::cell::RefCell;
    use std::iter;
    use std::mem;
    use std::rc::Rc;
    use std::sync::Arc;

    use gpui::Along;
    use gpui::AnyElement;
    use gpui::App;
    use gpui::Axis;
    use gpui::BorderStyle;
    use gpui::Bounds;
    use gpui::CursorStyle;
    use gpui::Element;
    use gpui::Entity;
    use gpui::GlobalElementId;
    use gpui::Hitbox;
    use gpui::HitboxBehavior;
    use gpui::IntoElement;
    use gpui::MouseDownEvent;
    use gpui::MouseMoveEvent;
    use gpui::MouseUpEvent;
    use gpui::ParentElement;
    use gpui::Pixels;
    use gpui::Point;
    use gpui::Size;
    use gpui::Style;
    use gpui::WeakEntity;
    use gpui::Window;
    use gpui::px;
    use gpui::relative;
    use gpui::size;
    use parking_lot::Mutex;
    use settings::Settings;
    use smallvec::SmallVec;
    use ui::Tab;
    use ui::prelude::*;
    use util::ResultExt;

    use super::HANDLE_HITBOX_SIZE;
    use super::HORIZONTAL_MIN_SIZE;
    use super::VERTICAL_MIN_SIZE;
    use crate::Pane;
    use crate::Workspace;
    use crate::WorkspaceSettings;

    /// Split panes are separate cards, with the chrome showing through the gap between them.
    const GAP: Pixels = crate::CARD_GAP;

    /// The space shared out between the children, excluding the gaps between them.
    fn children_size(bounds_size: Size<Pixels>, axis: Axis, len: usize) -> Size<Pixels> {
        bounds_size.apply_along(axis, |size| (size - GAP * len.saturating_sub(1) as f32).max(px(0.)))
    }

    pub(super) fn pane_axis(
        axis: Axis,
        basis: usize,
        flexes: Arc<Mutex<Vec<f32>>>,
        bounding_boxes: Arc<Mutex<Vec<Option<Bounds<Pixels>>>>>,
        workspace: WeakEntity<Workspace>,
    ) -> PaneAxisElement {
        PaneAxisElement {
            axis,
            basis,
            flexes,
            bounding_boxes,
            children: SmallVec::new(),
            active_pane_ix: None,
            workspace,
            leaf_panes: Vec::new(),
        }
    }

    pub struct PaneAxisElement {
        axis: Axis,
        basis: usize,
        /// Equivalent to ColumnWidths (but in terms of flexes instead of percentages)
        /// For example, flexes "1.33, 1, 1", instead of "40%, 30%, 30%"
        flexes: Arc<Mutex<Vec<f32>>>,
        bounding_boxes: Arc<Mutex<Vec<Option<Bounds<Pixels>>>>>,
        children: SmallVec<[AnyElement; 2]>,
        active_pane_ix: Option<usize>,
        workspace: WeakEntity<Workspace>,
        // The pane of each child that is a leaf (Member::Pane), `None` for axes (Member::Axis)
        leaf_panes: Vec<Option<Entity<Pane>>>,
    }

    pub struct PaneAxisLayout {
        dragged_handle: Rc<RefCell<Option<usize>>>,
        children: Vec<PaneAxisChildLayout>,
    }

    struct PaneAxisChildLayout {
        bounds: Bounds<Pixels>,
        element: AnyElement,
        handle: Option<PaneAxisHandleLayout>,
        /// For a leaf pane, its card: the pane's bounds below its tab bar.
        card_bounds: Option<Bounds<Pixels>>,
    }

    struct PaneAxisHandleLayout {
        hitbox: Hitbox,
    }

    impl PaneAxisElement {
        pub const fn with_active_pane(mut self, active_pane_ix: Option<usize>) -> Self {
            self.active_pane_ix = active_pane_ix;
            self
        }

        pub fn with_leaf_panes(mut self, leaf_panes: Vec<Option<Entity<Pane>>>) -> Self {
            self.leaf_panes = leaf_panes;
            self
        }

        fn compute_resize(
            flexes: &Arc<Mutex<Vec<f32>>>,
            e: &MouseMoveEvent,
            ix: usize,
            axis: Axis,
            child_start: Point<Pixels>,
            container_size: Size<Pixels>,
            workspace: WeakEntity<Workspace>,
            window: &mut Window,
            cx: &mut App,
        ) {
            let min_size = match axis {
                Axis::Horizontal => px(HORIZONTAL_MIN_SIZE),
                Axis::Vertical => px(VERTICAL_MIN_SIZE),
            };
            let mut flexes = flexes.lock();
            debug_assert!(flex_values_in_bounds(flexes.as_slice()));

            // Math to convert a flex value to a pixel value
            let size = move |ix, flexes: &[f32]| container_size.along(axis) * (flexes[ix] / flexes.len() as f32);

            // Don't allow resizing to less than the minimum size, if elements are already too small
            if min_size - px(1.) > size(ix, flexes.as_slice()) {
                return;
            }

            // This is basically a "bucket" of pixel changes that need to be applied in response to this
            // mouse event. Probably a small, fractional number like 0.5 or 1.5 pixels
            let mut proposed_current_pixel_change =
                (e.position - child_start).along(axis) - size(ix, flexes.as_slice());

            // This takes a pixel change, and computes the flex changes that correspond to this pixel change
            // as well as the next one, for some reason
            let flex_changes = |pixel_dx, target_ix: usize, next: isize, flexes: &[f32]| {
                let flex_change = pixel_dx / container_size.along(axis);
                let current_target_flex = flexes[target_ix] + flex_change;
                let next_target_flex = flexes[(target_ix.cast_signed() + next) as usize] - flex_change;
                (current_target_flex, next_target_flex)
            };

            // Generate the list of flex successors, from the current index.
            // If you're dragging column 3 forward, out of 6 columns, then this code will produce [4, 5, 6]
            // If you're dragging column 3 backward, out of 6 columns, then this code will produce [2, 1, 0]
            let mut successors = iter::from_fn({
                let forward = proposed_current_pixel_change > px(0.);
                let mut ix_offset = 0;
                let len = flexes.len();
                move || {
                    let result = if forward {
                        (ix + 1 + ix_offset < len).then(|| ix + ix_offset)
                    } else {
                        (ix.cast_signed() - ix_offset.cast_signed() >= 0).then(|| ix - ix_offset)
                    };

                    ix_offset += 1;

                    result
                }
            });

            // Now actually loop over these, and empty our bucket of pixel changes
            while proposed_current_pixel_change.abs() > px(0.) {
                let Some(current_ix) = successors.next() else {
                    break;
                };

                let next_target_size =
                    Pixels::max(size(current_ix + 1, flexes.as_slice()) - proposed_current_pixel_change, min_size);

                let current_target_size = Pixels::max(
                    size(current_ix, flexes.as_slice()) + size(current_ix + 1, flexes.as_slice()) - next_target_size,
                    min_size,
                );

                let current_pixel_change = current_target_size - size(current_ix, flexes.as_slice());

                let (current_target_flex, next_target_flex) =
                    flex_changes(current_pixel_change, current_ix, 1, flexes.as_slice());

                flexes[current_ix] = current_target_flex;
                flexes[current_ix + 1] = next_target_flex;

                proposed_current_pixel_change -= current_pixel_change;
            }

            workspace.update(cx, |this, cx| this.serialize_workspace(window, cx)).log_err();
            cx.stop_propagation();
            window.refresh();
        }

        fn layout_handle(
            axis: Axis,
            pane_bounds: Bounds<Pixels>,
            window: &mut Window,
            _cx: &mut App,
        ) -> PaneAxisHandleLayout {
            // The handle covers the gap after the pane, plus a little either side.
            let handle_size = GAP + px(HANDLE_HITBOX_SIZE);
            let handle_bounds = Bounds {
                origin: pane_bounds
                    .origin
                    .apply_along(axis, |origin| origin + pane_bounds.size.along(axis) - px(HANDLE_HITBOX_SIZE / 2.)),
                size: pane_bounds.size.apply_along(axis, |_| handle_size),
            };

            PaneAxisHandleLayout { hitbox: window.insert_hitbox(handle_bounds, HitboxBehavior::BlockMouse) }
        }
    }

    impl IntoElement for PaneAxisElement {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    impl Element for PaneAxisElement {
        type RequestLayoutState = ();
        type PrepaintState = PaneAxisLayout;

        fn id(&self) -> Option<ElementId> {
            Some(self.basis.into())
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _global_id: Option<&GlobalElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (gpui::LayoutId, Self::RequestLayoutState) {
            let style = Style {
                flex_grow: 1.,
                flex_shrink: 1.,
                flex_basis: relative(0.).into(),
                size: size(relative(1.).into(), relative(1.).into()),
                ..Style::default()
            };
            (window.request_layout(style, None, cx), ())
        }

        fn prepaint(
            &mut self,
            global_id: Option<&GlobalElementId>,
            bounds: Bounds<Pixels>,
            _state: &mut Self::RequestLayoutState,
            window: &mut Window,
            cx: &mut App,
        ) -> PaneAxisLayout {
            let dragged_handle =
                window.with_element_state::<Rc<RefCell<Option<usize>>>, _>(global_id.unwrap(), |state, _cx| {
                    let state = state.unwrap_or_else(|| Rc::new(RefCell::new(None)));
                    (state.clone(), state)
                });
            let flexes = self.flexes.lock().clone();
            let len = self.children.len();
            debug_assert!(flexes.len() == len);
            debug_assert!(flex_values_in_bounds(flexes.as_slice()));

            let total_flex = len as f32;

            let mut origin = bounds.origin;
            let space_per_flex = children_size(bounds.size, self.axis, len).along(self.axis) / total_flex;

            let mut bounding_boxes = self.bounding_boxes.lock();
            bounding_boxes.clear();

            let mut layout = PaneAxisLayout { dragged_handle, children: Vec::new() };
            for (ix, mut child) in mem::take(&mut self.children).into_iter().enumerate() {
                let child_flex = flexes[ix];

                let child_size = bounds.size.apply_along(self.axis, |_| space_per_flex * child_flex).map(|d| d.round());

                let child_bounds = Bounds { origin, size: child_size };

                bounding_boxes.push(Some(child_bounds));
                child.layout_as_root(child_size.into(), window, cx);
                child.prepaint_at(origin, window, cx);

                origin = origin.apply_along(self.axis, |val| val + child_size.along(self.axis) + GAP);

                // Read after prepaint, so the pane has rendered this frame.
                let card_bounds = self.leaf_panes.get(ix).and_then(Option::as_ref).map(|pane| {
                    let mut card_bounds = child_bounds;
                    if pane.read(cx).tab_bar_displayed() {
                        let tab_bar_height = Tab::container_height(cx);
                        card_bounds.origin.y += tab_bar_height;
                        card_bounds.size.height = (card_bounds.size.height - tab_bar_height).max(px(0.));
                    }
                    card_bounds
                });

                layout.children.push(PaneAxisChildLayout {
                    bounds: child_bounds,
                    element: child,
                    handle: None,
                    card_bounds,
                })
            }

            for (ix, child_layout) in layout.children.iter_mut().enumerate() {
                if ix < len - 1 {
                    child_layout.handle = Some(Self::layout_handle(self.axis, child_layout.bounds, window, cx));
                }
            }

            layout
        }

        fn paint(
            &mut self,
            _id: Option<&GlobalElementId>,
            bounds: gpui::Bounds<ui::prelude::Pixels>,
            _: &mut Self::RequestLayoutState,
            layout: &mut Self::PrepaintState,
            window: &mut Window,
            cx: &mut App,
        ) {
            for child in &mut layout.children {
                child.element.paint(window, cx);
            }

            let overlay_opacity = WorkspaceSettings::get(None, cx)
                .active_pane_modifiers
                .inactive_opacity
                .map(|val| val.0.clamp(0.0, 1.0))
                .and_then(|val| (val <= 1.).then_some(val));

            let mut overlay_background = cx.theme().colors().editor_background;
            if let Some(opacity) = overlay_opacity {
                overlay_background.fade_out(opacity);
            }

            let overlay_border = WorkspaceSettings::get(None, cx)
                .active_pane_modifiers
                .border_size
                .and_then(|val| (val >= 0.).then_some(val));

            let len = layout.children.len();
            for (ix, child) in &mut layout.children.iter_mut().enumerate() {
                // Only the card is dimmed / outlined, not the tab bar on the chrome above it.
                if let Some(card_bounds) = child.card_bounds {
                    if overlay_opacity.is_some() && self.active_pane_ix != Some(ix) {
                        window.paint_quad(gpui::fill(card_bounds, overlay_background).corner_radii(crate::CARD_RADIUS));
                    }

                    if let Some(border) = overlay_border
                        && self.active_pane_ix == Some(ix)
                    {
                        window.paint_quad(gpui::quad(
                            card_bounds,
                            crate::CARD_RADIUS,
                            gpui::transparent_black(),
                            border,
                            cx.theme().colors().border_selected,
                            BorderStyle::Solid,
                        ));
                    }
                }

                if let Some(handle) = child.handle.as_mut() {
                    let cursor_style = match self.axis {
                        Axis::Vertical => CursorStyle::ResizeRow,
                        Axis::Horizontal => CursorStyle::ResizeColumn,
                    };

                    if layout.dragged_handle.borrow().is_some_and(|dragged_ix| dragged_ix == ix) {
                        window.set_window_cursor_style(cursor_style);
                    } else {
                        window.set_cursor_style(cursor_style, &handle.hitbox);
                    }

                    window.on_mouse_event({
                        let dragged_handle = layout.dragged_handle.clone();
                        let flexes = self.flexes.clone();
                        let workspace = self.workspace.clone();
                        let handle_hitbox = handle.hitbox.clone();
                        move |e: &MouseDownEvent, phase, window, cx| {
                            if phase.bubble() && handle_hitbox.is_hovered(window) {
                                dragged_handle.replace(Some(ix));
                                if e.click_count >= 2 {
                                    let mut borrow = flexes.lock();
                                    *borrow = vec![1.; borrow.len()];
                                    workspace.update(cx, |this, cx| this.serialize_workspace(window, cx)).log_err();

                                    window.refresh();
                                }
                                cx.stop_propagation();
                            }
                        }
                    });
                    window.on_mouse_event({
                        let workspace = self.workspace.clone();
                        let dragged_handle = layout.dragged_handle.clone();
                        let flexes = self.flexes.clone();
                        let child_bounds = child.bounds;
                        let axis = self.axis;
                        move |e: &MouseMoveEvent, phase, window, cx| {
                            let dragged_handle = dragged_handle.borrow();
                            if phase.bubble() && *dragged_handle == Some(ix) {
                                Self::compute_resize(
                                    &flexes,
                                    e,
                                    ix,
                                    axis,
                                    child_bounds.origin,
                                    children_size(bounds.size, axis, len),
                                    workspace.clone(),
                                    window,
                                    cx,
                                )
                            }
                        }
                    });
                }
            }

            window.on_mouse_event({
                let dragged_handle = layout.dragged_handle.clone();
                move |_: &MouseUpEvent, phase, _window, _cx| {
                    if phase.bubble() {
                        dragged_handle.replace(None);
                    }
                }
            });
        }
    }

    impl ParentElement for PaneAxisElement {
        fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
            self.children.extend(elements)
        }
    }

    fn flex_values_in_bounds(flexes: &[f32]) -> bool {
        (flexes.iter().copied().sum::<f32>() - flexes.len() as f32).abs() < 0.001
    }
}
