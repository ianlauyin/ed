use ed_actions::Quit;
use ed_actions::command_palette;
use ed_actions::git_panel;
use ed_actions::project_panel;
use editor::actions;
use gpui::App;
use gpui::Menu;
use gpui::MenuItem;
use gpui::OsAction;
use search::buffer_search;
use terminal_view::terminal_panel;

pub(super) fn app_menus(_cx: &mut App) -> Vec<Menu> {
    let mut view_items = vec![
        MenuItem::action("Zoom In", ed_actions::IncreaseBufferFontSize),
        MenuItem::action("Zoom Out", ed_actions::DecreaseBufferFontSize),
        MenuItem::action("Reset Zoom", ed_actions::ResetBufferFontSize),
        MenuItem::action("Reset All Zoom", ed_actions::ResetAllZoom),
        MenuItem::separator(),
        MenuItem::action("Cycle Left Dock", workspace::CycleLeftDock),
        MenuItem::action("Toggle Bottom Dock", workspace::ToggleBottomDock),
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
        MenuItem::action("Fold All", actions::FoldAll),
        MenuItem::action("Unfold All", actions::UnfoldAll),
        MenuItem::separator(),
        MenuItem::action("Project Panel", project_panel::ToggleFocus),
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
                MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide ed", super::Hide),
                MenuItem::action("Hide Others", super::HideOthers),
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
                MenuItem::action("Open…", workspace::Open::default()),
                MenuItem::action("Open Recent…", ed_actions::OpenRecent::default()),
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
                MenuItem::os_action("Undo", actions::Undo, OsAction::Undo),
                MenuItem::os_action("Redo", actions::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", actions::Cut, OsAction::Cut),
                MenuItem::os_action("Copy", actions::Copy, OsAction::Copy),
                MenuItem::os_action("Paste", actions::Paste, OsAction::Paste),
                MenuItem::separator(),
                MenuItem::action("Find", buffer_search::Deploy::find()),
                MenuItem::action("Find in Project", workspace::DeploySearch::default()),
                MenuItem::separator(),
                MenuItem::action("Toggle Line Comment", actions::ToggleComments::default()),
            ],
        },
        Menu {
            name: "Selection".into(),
            disabled: false,
            items: vec![
                MenuItem::os_action("Select All", actions::SelectAll, OsAction::SelectAll),
                MenuItem::action("Expand Selection", actions::SelectLargerSyntaxNode),
                MenuItem::action("Shrink Selection", actions::SelectSmallerSyntaxNode),
                MenuItem::action("Select Next Sibling", actions::SelectNextSyntaxNode),
                MenuItem::action("Select Previous Sibling", actions::SelectPreviousSyntaxNode),
                MenuItem::separator(),
                MenuItem::action("Add Cursor Above", actions::AddSelectionAbove { skip_soft_wrap: true }),
                MenuItem::action("Add Cursor Below", actions::AddSelectionBelow { skip_soft_wrap: true }),
                MenuItem::action("Select Next Occurrence", actions::SelectNext { replace_newest: false }),
                MenuItem::action("Select Previous Occurrence", actions::SelectPrevious { replace_newest: false }),
                MenuItem::action("Select All Occurrences", actions::SelectAllMatches),
                MenuItem::separator(),
                MenuItem::action("Move Line Up", actions::MoveLineUp),
                MenuItem::action("Move Line Down", actions::MoveLineDown),
                MenuItem::action("Duplicate Selection", actions::DuplicateLineDown),
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
                MenuItem::action("Command Palette...", command_palette::Toggle),
                MenuItem::separator(),
                MenuItem::action("Go to File...", workspace::ToggleFileFinder::default()),
                // MenuItem::action("Go to Symbol in Project", project_symbols::Toggle),
                MenuItem::action("Go to Line/Column...", actions::ToggleGoToLine),
                MenuItem::separator(),
                MenuItem::action("Go to Definition", actions::GoToDefinition::default()),
                MenuItem::action("Go to Declaration", actions::GoToDeclaration::default()),
                MenuItem::action("Go to Type Definition", actions::GoToTypeDefinition::default()),
                MenuItem::action("Find All References", actions::FindAllReferences::default()),
                MenuItem::separator(),
                MenuItem::action("Next Problem", actions::GoToDiagnostic::default()),
                MenuItem::action("Previous Problem", actions::GoToPreviousDiagnostic::default()),
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
