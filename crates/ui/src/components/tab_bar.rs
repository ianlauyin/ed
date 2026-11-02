use gpui::AnyElement;
use gpui::ScrollHandle;
use smallvec::SmallVec;

use crate::Tab;
use crate::TabStyle;
use crate::components::tab::FOLDER_TAB_RADIUS;
use crate::prelude::*;

#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    start_children: SmallVec<[AnyElement; 2]>,
    children: SmallVec<[AnyElement; 2]>,
    end_children: SmallVec<[AnyElement; 2]>,
    scroll_handle: Option<ScrollHandle>,
    style: TabStyle,
}

impl TabBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            start_children: SmallVec::new(),
            children: SmallVec::new(),
            end_children: SmallVec::new(),
            scroll_handle: None,
            style: TabStyle::Folder,
        }
    }

    pub const fn style(mut self, style: TabStyle) -> Self {
        self.style = style;
        self
    }

    pub fn track_scroll(mut self, scroll_handle: &ScrollHandle) -> Self {
        self.scroll_handle = Some(scroll_handle.clone());
        self
    }

    pub const fn start_children_mut(&mut self) -> &mut SmallVec<[AnyElement; 2]> {
        &mut self.start_children
    }

    pub fn start_child(mut self, start_child: impl IntoElement) -> Self
    where
        Self: Sized,
    {
        self.start_children_mut().push(start_child.into_element().into_any());
        self
    }

    pub fn start_children(mut self, start_children: impl IntoIterator<Item = impl IntoElement>) -> Self
    where
        Self: Sized,
    {
        self.start_children_mut().extend(start_children.into_iter().map(|child| child.into_any_element()));
        self
    }

    pub const fn end_children_mut(&mut self) -> &mut SmallVec<[AnyElement; 2]> {
        &mut self.end_children
    }

    pub fn end_child(mut self, end_child: impl IntoElement) -> Self
    where
        Self: Sized,
    {
        self.end_children_mut().push(end_child.into_element().into_any());
        self
    }

    pub fn end_children(mut self, end_children: impl IntoIterator<Item = impl IntoElement>) -> Self
    where
        Self: Sized,
    {
        self.end_children_mut().extend(end_children.into_iter().map(|child| child.into_any_element()));
        self
    }
}

impl ParentElement for TabBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

/// Without end buttons, a folder tab strip stops short of the card's rounded top-right corner, so a
/// tab cut off by it ends over the card's straight top edge instead of hanging past the curve.
fn end_inset(style: TabStyle, has_end_children: bool) -> Pixels {
    if style == TabStyle::Folder && !has_end_children { FOLDER_TAB_RADIUS } else { px(0.) }
}

impl RenderOnce for TabBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .group("tab_bar")
            .flex()
            .flex_none()
            .w_full()
            .h(Tab::container_height(cx))
            .map(|this| match self.style {
                // Clears the card's rounded top-left corner, so the first tab meets the card's
                // straight top edge.
                TabStyle::Folder => this.pl(FOLDER_TAB_RADIUS),
                // Pills sit inside the card, already inset from its edge.
                TabStyle::Pill => this.pl(DynamicSpacing::Base04.px(cx)),
            })
            .pr(end_inset(self.style, !self.end_children.is_empty()))
            .when(!self.start_children.is_empty(), |this| {
                this.child(
                    h_flex()
                        .flex_none()
                        .gap(DynamicSpacing::Base04.rems())
                        .px(DynamicSpacing::Base06.rems())
                        .children(self.start_children),
                )
            })
            .child(
                div().relative().flex_1().h_full().overflow_x_hidden().child(
                    h_flex()
                        .id("tabs")
                        .flex_grow_1()
                        .overflow_x_scroll()
                        .when_some(self.scroll_handle, |cx, scroll_handle| cx.track_scroll(&scroll_handle))
                        .children(self.children),
                ),
            )
            .when(!self.end_children.is_empty(), |this| {
                this.child(
                    h_flex()
                        .flex_none()
                        .gap(DynamicSpacing::Base04.rems())
                        .px(DynamicSpacing::Base06.rems())
                        .children(self.end_children),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_end_inset() {
        assert_eq!(end_inset(TabStyle::Folder, false), FOLDER_TAB_RADIUS);
        assert_eq!(end_inset(TabStyle::Folder, true), px(0.));
        assert_eq!(end_inset(TabStyle::Pill, false), px(0.));
    }
}
