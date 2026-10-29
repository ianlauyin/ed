use std::sync::Arc;

use gpui::AnyView;
use gpui::ClickEvent;
use gpui::SharedString;

use crate::IconButtonShape;
use crate::prelude::*;

#[derive(IntoElement)]
pub struct Disclosure {
    id: ElementId,
    is_open: bool,
    selected: bool,
    disabled: bool,
    closed_icon: IconName,
    shape: Option<IconButtonShape>,
    visible_on_hover: Option<SharedString>,
    #[allow(clippy::type_complexity)]
    tooltip: Option<Box<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>>,
    #[allow(clippy::type_complexity)]
    on_toggle_expanded: Option<Arc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    opened_icon: IconName,
}

impl Disclosure {
    pub fn new(id: impl Into<ElementId>, is_open: bool) -> Self {
        Self {
            id: id.into(),
            is_open,
            selected: false,
            disabled: false,
            on_toggle_expanded: None,
            opened_icon: IconName::ChevronDown,
            closed_icon: IconName::ChevronRight,
            shape: None,
            visible_on_hover: None,
            tooltip: None,
        }
    }

    pub fn tooltip(mut self, tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.tooltip = Some(Box::new(tooltip));
        self
    }

    pub fn on_toggle_expanded(
        mut self,
        handler: impl Into<Option<Arc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>>,
    ) -> Self {
        self.on_toggle_expanded = handler.into();
        self
    }

    pub const fn opened_icon(mut self, icon: IconName) -> Self {
        self.opened_icon = icon;
        self
    }

    pub const fn closed_icon(mut self, icon: IconName) -> Self {
        self.closed_icon = icon;
        self
    }

    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the shape of the underlying [`IconButton`].
    pub const fn shape(mut self, shape: IconButtonShape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Alias for [`Self::on_toggle_expanded`].
    pub fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle_expanded(Arc::new(handler) as Arc<_>)
    }
}

impl Toggleable for Disclosure {
    fn toggle_state(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl VisibleOnHover for Disclosure {
    fn visible_on_hover(mut self, group_name: impl Into<SharedString>) -> Self {
        self.visible_on_hover = Some(group_name.into());
        self
    }
}

impl RenderOnce for Disclosure {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        IconButton::new(
            self.id,
            match self.is_open {
                true => self.opened_icon,
                false => self.closed_icon,
            },
        )
        .icon_color(Color::Muted)
        .icon_size(IconSize::Small)
        .disabled(self.disabled)
        .when_some(self.shape, |this, shape| this.shape(shape))
        .toggle_state(self.selected)
        .when_some(self.visible_on_hover.clone(), |this, group_name| this.visible_on_hover(group_name))
        .when_some(self.tooltip, |this, tooltip| this.tooltip(tooltip))
        .when_some(self.on_toggle_expanded, move |this, on_toggle| {
            this.on_click(move |event, window, cx| on_toggle(event, window, cx))
        })
    }
}
