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
- Card content is derived from the tab bar colour instead of a new theme key:
  the editor, gutter, toolbar and terminal backgrounds are shifted together so
  the editor sits 5 points of HSL lightness below the tab bar (above it in light
  themes). The shift only ever adds contrast; themes that already have more are
  left alone. Opaque active-line colours shift too; translucent ones already
  follow the background.
- Tab bars and inactive tabs then take the card content colour, so the tab bar
  blends into the card. The active tab is 5 points lighter than the content (5
  points darker in light themes).
- Side docks (project, outline, git panels) have no border; they blend into the
  chrome.
- The center pane group and the bottom dock are cards:
  `editor_background` fill, a 1px `border_variant` border, and a 10px radius.
- A zoomed view (editor or any dock) is a card that covers exactly the area the
  cards occupy, overlaying the others, so every zoomed view has the same size.
  The `zoomed_padding` setting is removed.
- An 8px gap separates the cards from the window edges, from each other and from
  open docks. There is no gap under the title bar. When the status bar is shown,
  it replaces the bottom gap and pads its content evenly above and below, so the
  icons sit centred between the card and the window edge.
- Tabs are rounded and fill the tab bar's full height. The tab bar has no bottom
  line and no borders between tabs.
- GPUI only clips to rectangles. Cards inset their content by 2px at the bottom,
  and panes inset everything below the tab bar by 2px on each side. With the 1px
  border this keeps square content corners (editor background, scrollbars)
  inside the rounded edge. If the radius grows, the inset must grow so that
  border + inset >= radius × (1 − 1/√2).
- Tab bars have no inset and paint no background of their own (the card colour
  shows through), so tabs meet the card's top and side borders. Tab top corners
  use a 9px radius, matching the card's inner curve, so a tab in the card's
  corner sits flush. A pane with its tab bar hidden, or a non-pane panel docked
  at the bottom, would show square corners at the top.
