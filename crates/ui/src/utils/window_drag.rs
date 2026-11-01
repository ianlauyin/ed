use std::cell::Cell;

use gpui::Div;
use gpui::InteractiveElement as _;
use gpui::MouseButton;
use gpui::Stateful;
use gpui::StatefulInteractiveElement as _;
use gpui::WindowControlArea;

thread_local! {
    /// Set by a left press inside a drag area, consumed by the first drag
    /// that follows it. One mouse means one pending press is enough.
    static WINDOW_MOVE_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Makes `element` behave like a titlebar: dragging from it moves the window,
/// and a double click zooms or minimises it as the OS is set up to. Children
/// that handle the mouse themselves must stop mouse-down propagation.
pub fn window_drag_area(element: Stateful<Div>) -> Stateful<Div> {
    element
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |_, _, _| WINDOW_MOVE_PENDING.set(true))
        .on_mouse_up(MouseButton::Left, |_, _, _| WINDOW_MOVE_PENDING.set(false))
        .on_mouse_move(|event, window, _| {
            if take_window_move(event.dragging()) {
                window.start_window_move();
            }
        })
        .on_click(|event, window, _| {
            if event.click_count() == 2 {
                window.titlebar_double_click();
            }
        })
}

/// Whether a mouse move should start moving the window: only a drag that
/// follows a press inside a drag area, and only once per press.
fn take_window_move(dragging: bool) -> bool {
    dragging && WINDOW_MOVE_PENDING.replace(false)
}

#[cfg(test)]
mod tests {
    use super::WINDOW_MOVE_PENDING;
    use super::take_window_move;

    #[test]
    fn test_take_window_move() {
        WINDOW_MOVE_PENDING.set(false);
        assert!(!take_window_move(true));

        WINDOW_MOVE_PENDING.set(true);
        assert!(!take_window_move(false));
        assert!(take_window_move(true));
        assert!(!take_window_move(true));
    }
}
