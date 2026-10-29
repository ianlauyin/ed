use gpui::Anchor;
use gpui::AnyView;
use gpui::Entity;
use gpui::Pixels;
use gpui::Point;

use super::PopoverMenuHandle;
use crate::ButtonLike;
use crate::ContextMenu;
use crate::PopoverMenu;
use crate::prelude::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropdownStyle {
    #[default]
    Solid,
    Outlined,
    Subtle,
    Ghost,
}

enum LabelKind {
    Text(SharedString),
    Element(AnyElement),
}

#[derive(IntoElement)]
pub struct DropdownMenu {
    id: ElementId,
    label: LabelKind,
    trigger_size: ButtonSize,
    #[allow(clippy::type_complexity)]
    trigger_tooltip: Option<Box<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>>,
    trigger_icon: Option<IconName>,
    style: DropdownStyle,
    menu: Entity<ContextMenu>,
    full_width: bool,
    disabled: bool,
    handle: Option<PopoverMenuHandle<ContextMenu>>,
    attach: Option<Anchor>,
    offset: Option<Point<Pixels>>,
    tab_index: Option<isize>,
    chevron: bool,
}

impl DropdownMenu {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, menu: Entity<ContextMenu>) -> Self {
        Self {
            id: id.into(),
            label: LabelKind::Text(label.into()),
            trigger_size: ButtonSize::Default,
            trigger_tooltip: None,
            trigger_icon: Some(IconName::ChevronUpDown),
            style: DropdownStyle::default(),
            menu,
            full_width: false,
            disabled: false,
            handle: None,
            attach: None,
            offset: None,
            tab_index: None,
            chevron: true,
        }
    }

    pub fn new_with_element(id: impl Into<ElementId>, label: AnyElement, menu: Entity<ContextMenu>) -> Self {
        Self {
            id: id.into(),
            label: LabelKind::Element(label),
            trigger_size: ButtonSize::Default,
            trigger_tooltip: None,
            trigger_icon: Some(IconName::ChevronUpDown),
            style: DropdownStyle::default(),
            menu,
            full_width: false,
            disabled: false,
            handle: None,
            attach: None,
            offset: None,
            tab_index: None,
            chevron: true,
        }
    }

    pub const fn style(mut self, style: DropdownStyle) -> Self {
        self.style = style;
        self
    }

    pub const fn trigger_size(mut self, size: ButtonSize) -> Self {
        self.trigger_size = size;
        self
    }

    pub fn trigger_tooltip(mut self, tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.trigger_tooltip = Some(Box::new(tooltip));
        self
    }

    pub const fn trigger_icon(mut self, icon: IconName) -> Self {
        self.trigger_icon = Some(icon);
        self
    }

    pub const fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    pub fn handle(mut self, handle: PopoverMenuHandle<ContextMenu>) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Defines which corner of the handle to attach the menu's anchor to.
    pub const fn attach(mut self, attach: Anchor) -> Self {
        self.attach = Some(attach);
        self
    }

    /// Offsets the position of the menu by that many pixels.
    pub const fn offset(mut self, offset: Point<Pixels>) -> Self {
        self.offset = Some(offset);
        self
    }

    pub const fn tab_index(mut self, arg: isize) -> Self {
        self.tab_index = Some(arg);
        self
    }

    pub const fn no_chevron(mut self) -> Self {
        self.chevron = false;
        self
    }
}

impl Disableable for DropdownMenu {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for DropdownMenu {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let button_style = match self.style {
            DropdownStyle::Solid => ButtonStyle::Filled,
            DropdownStyle::Subtle => ButtonStyle::Subtle,
            DropdownStyle::Outlined => ButtonStyle::Outlined,
            DropdownStyle::Ghost => ButtonStyle::Transparent,
        };

        let full_width = self.full_width;
        let trigger_size = self.trigger_size;
        let handle = self.handle.unwrap_or_default();

        let (text_button, element_button) = match self.label {
            LabelKind::Text(text) => (
                Some(
                    Button::new(self.id.clone(), text)
                        .style(button_style)
                        .when_some(self.trigger_icon.filter(|_| self.chevron), |this, icon| {
                            this.end_icon(Icon::new(icon).size(IconSize::XSmall).color(Color::Muted))
                        })
                        .when(full_width, |this| this.full_width())
                        .size(trigger_size)
                        .disabled(self.disabled)
                        .when_some(self.tab_index, |this, tab_index| this.tab_index(tab_index)),
                ),
                None,
            ),
            LabelKind::Element(element) => (
                None,
                Some(
                    ButtonLike::new(self.id.clone())
                        .child(element)
                        .style(button_style)
                        .when(self.chevron, |this| {
                            this.child(Icon::new(IconName::ChevronUpDown).size(IconSize::XSmall).color(Color::Muted))
                        })
                        .when(full_width, |this| this.full_width())
                        .size(trigger_size)
                        .disabled(self.disabled)
                        .when_some(self.tab_index, |this, tab_index| this.tab_index(tab_index)),
                ),
            ),
        };

        // When the menu opens, move the selection to the current value (or the
        // first item) so assistive technology announces a meaningful item
        // immediately, instead of focusing the bare menu container and
        // announcing only "menu". See the ARIA menu button pattern.
        let menu_for_open = self.menu.clone();
        let mut popover = PopoverMenu::new((self.id.clone(), "popover"))
            .full_width(self.full_width)
            .with_handle(handle)
            .on_open(std::rc::Rc::new(move |window, cx| {
                menu_for_open.update(cx, |menu, cx| {
                    menu.select_toggled_or_first(window, cx);
                });
            }))
            .menu(move |_window, _cx| Some(self.menu.clone()));

        popover = match (text_button, element_button, self.trigger_tooltip) {
            (Some(text_button), None, Some(tooltip)) => popover.trigger_with_tooltip(text_button, tooltip),
            (Some(text_button), None, None) => popover.trigger(text_button),
            (None, Some(element_button), Some(tooltip)) => popover.trigger_with_tooltip(element_button, tooltip),
            (None, Some(element_button), None) => popover.trigger(element_button),
            _ => popover,
        };

        popover
            .attach(match self.attach {
                Some(attach) => attach,
                None => Anchor::BottomRight,
            })
            .when_some(self.offset, |this, offset| this.offset(offset))
    }
}
