- no support for hard click touchpad (lookup), add or put right click menu

# rethink pane left/right short key
  - cmd+J, terminal/central terminal behavior
  - always use cmd+B cmd+R, or 

- add webkit tab to render html? still need markdown html preview? or just use webkit to open html, then how to open remote html? 

# think about keymap/shortcut key, 
  - cmd+enter = expand window
  - keymap, construct directly rather than parse json, (simplify Deserialize on actions)
 
- remove settingContent entirely? just hard code into code

- add workspace scoped UI settings, like title or border color, do i really need this? to distinguish project

- add complete rust / clippy lint

- review async crates, replace smol with tokio? (as smol dev seems to be stopped)

- think about remote "download" vs local "open in default app", copy/paste file across local/remote

- think about if we need external terminal, just do everything in ed, introduce terminal mode?

# should add low power mode? or my laptop is powerful enough
- Frame pacing (#63477, #66237): large changes to window.rs, and the macOS Low Power Mode part drags in Linux and Windows power code. I’d skip them unless you’ve noticed stutter in Low Power Mode.

# asset
- is it good idea to embed assets into binary? should it be side loaded, like from file/folder
