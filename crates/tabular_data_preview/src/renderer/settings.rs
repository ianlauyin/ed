use gpui::Anchor;
use gpui::Entity;
use ui::ContextMenu;
use ui::ContextMenuEntry;
use ui::DocumentationSide;
use ui::IconButton;
use ui::IconName;
use ui::IconPosition;
use ui::IconSize;
use ui::Label;
use ui::PopoverMenu;
use ui::Tooltip;
use ui::prelude::*;

use crate::TableView;
use crate::settings::FilterSortOrder;
use crate::settings::TableViewSettings;
use crate::settings::VerticalAlignment;

///// Settings related /////

/// Adds a toggleable entry that applies `set` to the pane's settings when clicked. `description`,
/// when given, shows as a documentation aside explaining what the entry does.
fn toggle_entry(
    menu: ContextMenu,
    label: &'static str,
    description: Option<&'static str>,
    selected: bool,
    view_entity: &Entity<TableView>,
    set: impl Fn(&mut TableViewSettings) + 'static,
) -> ContextMenu {
    let view_entity = view_entity.clone();
    let entry = ContextMenuEntry::new(label).toggleable(IconPosition::Start, selected).handler(move |_, cx| {
        view_entity.update(cx, |this, cx| {
            set(&mut this.settings);
            cx.notify();
        });
    });
    let entry = if let Some(description) = description {
        entry.documentation_aside(DocumentationSide::Right, move |_| Label::new(description).into_any_element())
    } else {
        entry
    };
    menu.item(entry)
}

pub(crate) fn settings_popover_menu(view_entity: Entity<TableView>) -> PopoverMenu<ContextMenu> {
    PopoverMenu::new("table-settings-menu")
        .trigger_with_tooltip(
            IconButton::new("table-settings-trigger", IconName::Filter)
                .icon_size(IconSize::Small)
                .size(ButtonSize::Compact),
            Tooltip::text("Table Settings"),
        )
        .anchor(Anchor::TopRight)
        .menu(move |window, cx| {
            let view_entity = view_entity.clone();
            Some(ContextMenu::build_persistent(window, cx, move |menu, _window, cx| {
                let settings = view_entity.read(cx).settings.clone();

                let menu = toggle_entry(
                    menu.header("Text Alignment"),
                    "Top",
                    Some("Choose vertical text alignment within cells"),
                    matches!(settings.vertical_alignment, VerticalAlignment::Top),
                    &view_entity,
                    |settings| settings.vertical_alignment = VerticalAlignment::Top,
                );
                let menu = toggle_entry(
                    menu,
                    "Center",
                    None,
                    matches!(settings.vertical_alignment, VerticalAlignment::Center),
                    &view_entity,
                    |settings| settings.vertical_alignment = VerticalAlignment::Center,
                );

                let menu = menu.separator().header("Filter Sort");
                let menu = toggle_entry(
                    menu,
                    "A-Z, then Count",
                    Some("Choose how filter values are sorted in the filter menu"),
                    settings.filter_sort_order == FilterSortOrder::AlphaThenCount,
                    &view_entity,
                    |settings| settings.filter_sort_order = FilterSortOrder::AlphaThenCount,
                );
                let menu = toggle_entry(
                    menu,
                    "Count, then A-Z",
                    None,
                    settings.filter_sort_order == FilterSortOrder::CountThenAlpha,
                    &view_entity,
                    |settings| settings.filter_sort_order = FilterSortOrder::CountThenAlpha,
                );

                toggle_entry(
                    menu.separator(),
                    "Display multiline rows",
                    Some(
                        "When enabled, row height grows to show all content. \
                             When disabled, only the first line is visible — hover a cell to see the rest.",
                    ),
                    settings.multiline_cells_enabled,
                    &view_entity,
                    |settings| settings.multiline_cells_enabled = !settings.multiline_cells_enabled,
                )
            }))
        })
}
