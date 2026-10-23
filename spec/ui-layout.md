# UI layout

## Decision

The editor area and the terminal float as opaque rounded cards on a single
opaque chrome colour. It is always on, with no setting, and is done the simplest
way: theme colour tweaks plus a shared card style, with no changes to GPUI's
clipping. A translucent, blurred liquid-glass variant was tried and dropped,
because it hurt readability; an opaque window is also cheaper to composite.

## Behaviour

- The chrome is the active theme's `panel_background`. Before user theme
  overrides run, `background`, the title bar colours and `status_bar_background`
  are set to it. User `theme_overrides` can still change any of these.
- Card content is derived from the chrome colour instead of a new theme key:
  the editor, gutter, toolbar and terminal backgrounds are shifted together so
  the editor sits 5 points of HSL lightness below the chrome (above it in light
  themes). The shift only ever adds contrast; themes that already have more are
  left alone. Opaque active-line colours shift too; translucent ones already
  follow the background.
- Each pane is its own card: its tab bar sits on the chrome, and everything
  below the tab bar is the card (`editor_background` fill, 10px radius, no
  border). Tab bars and inactive tabs take the chrome colour, so only the active
  tab stands out: it takes the card colour and joins the card below it, like a
  browser tab. This replaced cards that wrapped the tab bar, where inactive tabs
  shared the card colour and looked continuous with the active one.
- Docks have no border and blend into the chrome. Panes inside them (the
  terminal) draw their own cards; a non-pane panel docked at the bottom just
  sits on the chrome.
- Split panes are separate cards with an 8px gap between them (the chrome shows
  through) instead of a divider line. The resize handle covers the gap.
- Inactive-pane dimming (`active_pane_modifiers.inactive_opacity`) and the
  active-pane border cover only the rounded card, never the tab bar, so tab
  bars stay the plain chrome colour in every pane.
- A zoomed view (editor or any dock) covers exactly the area the cards occupy,
  on the chrome, overlaying the others, so every zoomed view has the same size.
  The `zoomed_padding` setting is removed.
- An 8px gap separates the cards from the window edges and from open docks.
  There is no gap under the title bar. When the status bar is shown, it replaces
  the bottom gap and pads its content evenly above and below, so the icons sit
  centred between the card and the window edge.
- Status bar items have no "Hide Button" right-click entry; they are hidden only
  through their settings. Items without other menu entries (search, cursor
  position, language, diagnostics, …) have no right-click menu at all; dock
  panel buttons keep theirs for dock position only.
- Docks always have a fixed size in pixels (width for left/right, height for
  bottom). There is no proportional "flex width" mode and no
  `terminal.flexible` setting. A side dock may shrink when space is tight but
  springs back to its stored width.
- Tabs fill the tab bar's full height with rounded top corners and square
  bottoms. The tab bar has no bottom line and no borders between tabs. It is
  inset 10px on the left (the card radius), so the first tab meets the card's
  straight top edge rather than its rounded corner.
- The terminal panel's panes use pill tabs instead: the whole pane, tab bar
  included, is one card, and tabs are rounded buttons inside its top edge.
  Only the active pill has a background (`ghost_element_selected`); inactive
  ones show `ghost_element_hover` on hover. Split drop zones cover only the
  content below the pills, and inactive-pane dimming covers the whole card.
- Pane toolbars (search bars, breadcrumbs) have no bottom line either; they sit
  flat inside the card.
- An empty pane (hints or welcome page) leaves a blank strip of tab-bar height
  above its card, so the card does not jump when the first tab opens or the
  last one closes.
- An empty pane keeps its card and shows centred keybinding hints (Open File,
  Go to File, Command Palette), styled like the project search landing page.
  Bindings are read from the keymap, and the welcome page still takes
  precedence when no project is open.
- GPUI only clips to rectangles, so the card insets its content by 3px on every
  side, keeping square content corners (editor background, scrollbars) inside
  the rounded edge. If the radius grows, the inset must grow so that
  inset >= radius × (1 − 1/√2).
- Symbols in the outline panel and the editor's outline modal render in the UI
  font at the project panel's label size, not the buffer font. Syntax highlight
  colours are kept.
