- no support for hard click touchpad (lookup), add or put right click menu
- rethink cmd+J, terminal/central terminal behavior
- add webkit tab to render html? still need markdown html preview? or just use webkit to open html, then how to open remote html? 

- think about keymap/shortcut key, only need simple ones (which_key retired), cmd+enter = expand window
 
- remove settingContent entirely? just hard code into code
- add workspace scoped UI settings, like title or border color, do i really need this? to distinguish project
- add complete rust / clippy lint
- review async crates, replace smol with tokio? (as smol dev seems to be stopped)
- keymap, construct directly rather than parse json, (simplify Deserialize on actions)

- think about remote "download" vs local "open in default app", copy/paste file across local/remote
- think about if we don't need external terminal, just do everything in ed?
- append external crate file in project file tree, and make them readonly (not allow to edit)
