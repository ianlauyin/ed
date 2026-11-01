use std::cmp::Ordering;

use gpui::AnyElement;
use gpui::Hsla;
use gpui::IntoElement;
use gpui::Stateful;
use smallvec::SmallVec;

use crate::prelude::*;

const START_TAB_SLOT_SIZE: Pixels = px(12.);
const END_TAB_SLOT_SIZE: Pixels = px(14.);
/// Matches `workspace::CARD_RADIUS`, so the tab, its flares and the card's corners all share one curve.
pub(crate) const FOLDER_TAB_RADIUS: Pixels = px(10.);

/// The position of a [`Tab`] within a list of tabs.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TabPosition {
    /// The tab is first in the list.
    First,

    /// The tab is in the middle of the list (i.e., it is not the first or last tab).
    ///
    /// The [`Ordering`] is where this tab is positioned with respect to the selected tab.
    Middle(Ordering),

    /// The tab is last in the list.
    Last,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TabCloseSide {
    Start,
    End,
}

/// How a [`Tab`] (and its [`TabBar`](crate::TabBar)) is drawn.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum TabStyle {
    /// Fills the tab bar height with rounded tops; the active tab joins the card below.
    #[default]
    Folder,
    /// A rounded button inside the card; the active one has full-contrast text and no fill.
    Pill,
}

#[derive(IntoElement)]
pub struct Tab {
    div: Stateful<Div>,
    selected: bool,
    style: TabStyle,
    floating: bool,
    position: TabPosition,
    close_side: TabCloseSide,
    start_slot: Option<AnyElement>,
    end_slot: Option<AnyElement>,
    children: SmallVec<[AnyElement; 2]>,
    overlays: SmallVec<[AnyElement; 2]>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            div: div().id(id.into()),
            selected: false,
            style: TabStyle::Folder,
            floating: false,
            position: TabPosition::First,
            close_side: TabCloseSide::End,
            start_slot: None,
            end_slot: None,
            children: SmallVec::new(),
            overlays: SmallVec::new(),
        }
    }

    pub const fn position(mut self, position: TabPosition) -> Self {
        self.position = position;
        self
    }

    pub const fn style(mut self, style: TabStyle) -> Self {
        self.style = style;
        self
    }

    /// Drawn away from a tab bar (a drag preview): no base flares, and an inactive tab keeps its fill.
    pub const fn floating(mut self) -> Self {
        self.floating = true;
        self
    }

    pub const fn close_side(mut self, close_side: TabCloseSide) -> Self {
        self.close_side = close_side;
        self
    }

    pub fn start_slot<E: IntoElement>(mut self, element: impl Into<Option<E>>) -> Self {
        self.start_slot = element.into().map(IntoElement::into_any_element);
        self
    }

    pub fn end_slot<E: IntoElement>(mut self, element: impl Into<Option<E>>) -> Self {
        self.end_slot = element.into().map(IntoElement::into_any_element);
        self
    }

    /// Positioned against the tab's shape (it is `relative`) and painted behind its content,
    /// e.g. a drop indicator.
    pub fn overlay(mut self, overlay: impl IntoElement) -> Self {
        self.overlays.push(overlay.into_any_element());
        self
    }

    /// The gap between neighbouring tabs of `style`.
    pub fn spacing(style: TabStyle, cx: &App) -> Pixels {
        match style {
            TabStyle::Folder => px(2.),
            TabStyle::Pill => DynamicSpacing::Base04.px(cx),
        }
    }

    pub fn content_height(cx: &App) -> Pixels {
        Tab::container_height(cx) - px(1.)
    }

    pub fn container_height(cx: &App) -> Pixels {
        DynamicSpacing::Base32.px(cx) + DynamicSpacing::Base04.px(cx)
    }

    pub fn pill_height(cx: &App) -> Pixels {
        DynamicSpacing::Base24.px(cx)
    }
}

impl InteractiveElement for Tab {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.div.interactivity()
    }
}

impl StatefulInteractiveElement for Tab {}

impl Toggleable for Tab {
    fn toggle_state(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl ParentElement for Tab {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl RenderOnce for Tab {
    #[allow(refining_impl_trait)]
    fn render(self, _: &mut Window, cx: &mut App) -> Stateful<Div> {
        let (text_color, tab_bg, _tab_hover_bg, _tab_active_bg) = match self.selected {
            false => (
                cx.theme().colors().text_muted,
                cx.theme().colors().tab_inactive_background,
                cx.theme().colors().ghost_element_hover,
                cx.theme().colors().ghost_element_active,
            ),
            true => (
                cx.theme().colors().text,
                cx.theme().colors().tab_active_background,
                cx.theme().colors().element_hover,
                cx.theme().colors().element_active,
            ),
        };

        let (start_slot, end_slot) = {
            let start_slot = h_flex().size(START_TAB_SLOT_SIZE).justify_center().children(self.start_slot);

            let end_slot = h_flex().size(END_TAB_SLOT_SIZE).justify_center().children(self.end_slot);

            match self.close_side {
                TabCloseSide::End => (start_slot, end_slot),
                TabCloseSide::Start => (end_slot, start_slot),
            }
        };

        if self.style == TabStyle::Pill {
            // Selection is shown by text contrast (and the item's own icon colour), not a fill,
            // matching the title bar's mode switch.
            let text_color = if self.selected { cx.theme().colors().text } else { cx.theme().colors().text_muted };
            let hover_bg = cx.theme().colors().ghost_element_hover;
            return self
                .div
                .h(Tab::container_height(cx))
                .flex()
                .items_center()
                .pr(Tab::spacing(TabStyle::Pill, cx))
                .cursor_pointer()
                .child(
                    h_flex()
                        .group("")
                        .relative()
                        .h(Tab::pill_height(cx))
                        .rounded_full()
                        .hover(|s| s.bg(hover_bg))
                        .px(DynamicSpacing::Base08.px(cx))
                        .gap(DynamicSpacing::Base04.rems())
                        .text_color(text_color)
                        .children(self.overlays)
                        .child(start_slot)
                        .children(self.children)
                        .child(end_slot),
                );
        }

        // Tabs fill the tab bar height with rounded tops; the active one joins the card below,
        // flaring into it. Inactive tabs are the bar's colour, so they paint no fill that would
        // cover a neighbouring flare.
        let flares = self.selected && !self.floating;
        let chrome_bg = cx.theme().colors().tab_bar_background;
        self.div
            .h(Tab::container_height(cx))
            .flex()
            .items_center()
            .pl(folder_lead(self.position, self.floating))
            .pr(Tab::spacing(TabStyle::Folder, cx))
            .cursor_pointer()
            .child(
                h_flex()
                    .group("")
                    .relative()
                    .h_full()
                    .rounded_t(FOLDER_TAB_RADIUS)
                    .when(self.selected || self.floating, |this| this.bg(tab_bg))
                    .px(DynamicSpacing::Base12.px(cx))
                    .gap(DynamicSpacing::Base04.rems())
                    .text_color(text_color)
                    .when(flares, |this| {
                        this.child(base_flare(true, tab_bg, chrome_bg)).child(base_flare(false, tab_bg, chrome_bg))
                    })
                    .children(self.overlays)
                    .child(start_slot)
                    .children(self.children)
                    .child(end_slot),
            )
    }
}

/// Room before the first folder tab for its start flare, which with the card's rounded corner
/// below makes an S-curve.
fn folder_lead(position: TabPosition, floating: bool) -> Pixels {
    if position == TabPosition::First && !floating { FOLDER_TAB_RADIUS } else { px(0.) }
}

/// A concave curve beside the base of the active folder tab, joining it to the card's top edge:
/// a tab-coloured square under a chrome-coloured one rounded on its corner nearest the tab.
fn base_flare(at_start: bool, tab_bg: Hsla, chrome_bg: Hsla) -> Div {
    let flare = div().absolute().bottom_0().size(FOLDER_TAB_RADIUS).bg(tab_bg);
    let mask = div().size_full().bg(chrome_bg);
    if at_start {
        flare.left(-FOLDER_TAB_RADIUS).child(mask.rounded_br(FOLDER_TAB_RADIUS))
    } else {
        flare.right(-FOLDER_TAB_RADIUS).child(mask.rounded_bl(FOLDER_TAB_RADIUS))
    }
}

#[cfg(test)]
mod tests {
    use gpui::Length;

    use super::*;

    #[test]
    fn test_base_flare() {
        let (tab_bg, chrome_bg) = (gpui::red(), gpui::blue());
        let offset = Some(Length::from(-FOLDER_TAB_RADIUS));

        let mut start = base_flare(true, tab_bg, chrome_bg);
        let style = start.style();
        assert_eq!(style.background, Some(tab_bg.into()));
        assert_eq!(style.inset.left, offset);
        assert_eq!(style.inset.right, None);
        assert_eq!(style.inset.bottom, Some(Length::from(px(0.))));
        assert_eq!(style.size.width, Some(Length::from(FOLDER_TAB_RADIUS)));
        assert_eq!(style.size.height, Some(Length::from(FOLDER_TAB_RADIUS)));

        let mut end = base_flare(false, tab_bg, chrome_bg);
        let style = end.style();
        assert_eq!(style.inset.right, offset);
        assert_eq!(style.inset.left, None);
    }

    #[test]
    fn test_folder_lead() {
        assert_eq!(folder_lead(TabPosition::First, false), FOLDER_TAB_RADIUS);
        assert_eq!(folder_lead(TabPosition::First, true), px(0.));
        assert_eq!(folder_lead(TabPosition::Middle(Ordering::Less), false), px(0.));
        assert_eq!(folder_lead(TabPosition::Last, false), px(0.));
    }

    #[test]
    fn test_floating() {
        assert!(!Tab::new("tab").floating);
        assert!(Tab::new("tab").floating().floating);
    }
}
