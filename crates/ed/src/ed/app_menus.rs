use ed_actions::Quit;
use ed_actions::git_panel;
use ed_actions::project_panel;
use gpui::App;
use gpui::Menu;
use gpui::MenuItem;
use gpui::OsAction;
use terminal_view::terminal_panel;

pub fn app_menus(_cx: &mut App) -> Vec<Menu> {
    let mut view_items = vec![
        MenuItem::action("Zoom In", ed_actions::IncreaseBufferFontSize { persist: false }),
        MenuItem::action("Zoom Out", ed_actions::DecreaseBufferFontSize { persist: false }),
        MenuItem::action("Reset Zoom", ed_actions::ResetBufferFontSize { persist: false }),
        MenuItem::action("Reset All Zoom", ed_actions::ResetAllZoom { persist: false }),
        MenuItem::separator(),
        MenuItem::action("Toggle Left Dock", workspace::ToggleLeftDock),
        MenuItem::action("Toggle Right Dock", workspace::ToggleRightDock),
        MenuItem::action("Toggle Bottom Dock", workspace::ToggleBottomDock),
        MenuItem::action("Toggle All Docks", workspace::ToggleAllDocks),
        MenuItem::submenu(Menu {
            name: "Editor Layout".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Split Up", workspace::SplitUp::default()),
                MenuItem::action("Split Down", workspace::SplitDown::default()),
                MenuItem::action("Split Left", workspace::SplitLeft::default()),
                MenuItem::action("Split Right", workspace::SplitRight::default()),
            ],
        }),
        MenuItem::separator(),
        MenuItem::action("Fold All", editor::actions::FoldAll),
        MenuItem::action("Unfold All", editor::actions::UnfoldAll),
        MenuItem::separator(),
        MenuItem::action("Project Panel", project_panel::ToggleFocus),
        MenuItem::action("Outline Panel", outline_panel::ToggleFocus),
        MenuItem::action("Terminal Panel", terminal_panel::Toggle),
    ];

    view_items.extend([
        MenuItem::action("Git Panel", git_panel::ToggleFocus),
        MenuItem::separator(),
        MenuItem::action("Diagnostics", diagnostics::Deploy),
        MenuItem::separator(),
    ]);

    vec![
        Menu {
            name: "ed".into(),
            disabled: false,
            items: vec![
                MenuItem::action("About ed", ed_actions::About),
                MenuItem::separator(),
                MenuItem::submenu(
                    Menu::new("Settings")
                        .items([MenuItem::action("Select Theme...", ed_actions::theme_selector::Toggle::default())]),
                ),
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::action("Hide ed", super::Hide),
                #[cfg(target_os = "macos")]
                MenuItem::action("Hide Others", super::HideOthers),
                #[cfg(target_os = "macos")]
                MenuItem::action("Show All", super::ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit ed", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            disabled: false,
            items: vec![
                MenuItem::action("New", workspace::NewFile),
                MenuItem::action("New Window", workspace::NewWindow),
                MenuItem::separator(),
                #[cfg(not(target_os = "macos"))]
                MenuItem::action("Open File...", workspace::OpenFiles),
                MenuItem::action(
                    if cfg!(not(target_os = "macos")) { "Open Folder..." } else { "Open…" },
                    workspace::Open::default(),
                ),
                MenuItem::action("Open Recent…", ed_actions::OpenRecent::default()),
                MenuItem::action("Open Remote…", ed_actions::OpenRemote::default()),
                MenuItem::separator(),
                MenuItem::action("Add Folder to Project…", workspace::AddFolderToProject),
                MenuItem::separator(),
                MenuItem::action("Save", workspace::Save { save_intent: None }),
                MenuItem::action("Save As…", workspace::SaveAs),
                MenuItem::action("Save All", workspace::SaveAll { save_intent: None }),
                MenuItem::separator(),
                MenuItem::action("Close Editor", workspace::CloseActiveItem { save_intent: None, close_pinned: true }),
                MenuItem::action("Close Project", workspace::CloseProject),
                MenuItem::action("Close Window", workspace::CloseWindow),
            ],
        },
        Menu {
            name: "Edit".into(),
            disabled: false,
            items: vec![
                MenuItem::os_action("Undo", editor::actions::Undo, OsAction::Undo),
                MenuItem::os_action("Redo", editor::actions::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", editor::actions::Cut, OsAction::Cut),
                MenuItem::os_action("Copy", editor::actions::Copy, OsAction::Copy),
                MenuItem::action("Copy and Trim", editor::actions::CopyAndTrim),
                MenuItem::os_action("Paste", editor::actions::Paste, OsAction::Paste),
                MenuItem::separator(),
                MenuItem::action("Find", search::buffer_search::Deploy::find()),
                MenuItem::action("Find in Project", workspace::DeploySearch::default()),
                MenuItem::separator(),
                MenuItem::action("Toggle Line Comment", editor::actions::ToggleComments::default()),
            ],
        },
        Menu {
            name: "Selection".into(),
            disabled: false,
            items: vec![
                MenuItem::os_action("Select All", editor::actions::SelectAll, OsAction::SelectAll),
                MenuItem::action("Expand Selection", editor::actions::SelectLargerSyntaxNode),
                MenuItem::action("Shrink Selection", editor::actions::SelectSmallerSyntaxNode),
                MenuItem::action("Select Next Sibling", editor::actions::SelectNextSyntaxNode),
                MenuItem::action("Select Previous Sibling", editor::actions::SelectPreviousSyntaxNode),
                MenuItem::separator(),
                MenuItem::action("Add Cursor Above", editor::actions::AddSelectionAbove { skip_soft_wrap: true }),
                MenuItem::action("Add Cursor Below", editor::actions::AddSelectionBelow { skip_soft_wrap: true }),
                MenuItem::action("Select Next Occurrence", editor::actions::SelectNext { replace_newest: false }),
                MenuItem::action(
                    "Select Previous Occurrence",
                    editor::actions::SelectPrevious { replace_newest: false },
                ),
                MenuItem::action("Select All Occurrences", editor::actions::SelectAllMatches),
                MenuItem::separator(),
                MenuItem::action("Move Line Up", editor::actions::MoveLineUp),
                MenuItem::action("Move Line Down", editor::actions::MoveLineDown),
                MenuItem::action("Duplicate Selection", editor::actions::DuplicateLineDown),
            ],
        },
        Menu { name: "View".into(), disabled: false, items: view_items },
        Menu {
            name: "Go".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Back", workspace::GoBack),
                MenuItem::action("Forward", workspace::GoForward),
                MenuItem::separator(),
                MenuItem::action("Command Palette...", ed_actions::command_palette::Toggle),
                MenuItem::separator(),
                MenuItem::action("Go to File...", workspace::ToggleFileFinder::default()),
                // MenuItem::action("Go to Symbol in Project", project_symbols::Toggle),
                MenuItem::action("Go to Line/Column...", editor::actions::ToggleGoToLine),
                MenuItem::separator(),
                MenuItem::action("Go to Definition", editor::actions::GoToDefinition::default()),
                MenuItem::action("Go to Declaration", editor::actions::GoToDeclaration::default()),
                MenuItem::action("Go to Type Definition", editor::actions::GoToTypeDefinition::default()),
                MenuItem::action("Find All References", editor::actions::FindAllReferences::default()),
                MenuItem::separator(),
                MenuItem::action("Next Problem", editor::actions::GoToDiagnostic::default()),
                MenuItem::action("Previous Problem", editor::actions::GoToPreviousDiagnostic::default()),
            ],
        },
        Menu {
            name: "Window".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Minimize", super::Minimize),
                MenuItem::action("Zoom", super::Zoom),
                MenuItem::separator(),
            ],
        },
    ]
}
