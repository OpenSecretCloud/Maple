//! The application menu bar. Every item dispatches the same action a
//! shortcut does, so a menu entry, its keyboard hint, and the Settings
//! shortcuts page cannot disagree. The Edit menu binds the platform's
//! standard actions so text fields and the transcript get Cut, Copy,
//! Paste, and Select All from the bar and from Services.

use gpui::{App, Menu, MenuItem, OsAction, SystemMenuType};

use super::chat;
use super::text_input;

/// Install the menu bar. gpui shows it on macOS; other platforms ignore
/// it, so the call is unconditional.
pub fn install(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Maple".into(),
            items: vec![
                MenuItem::action("Settings…", chat::OpenAppSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit Maple", crate::desktop::QuitApp),
            ],
            disabled: false,
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Undo", text_input::Undo, OsAction::Undo),
                MenuItem::os_action("Redo", text_input::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", text_input::Cut, OsAction::Cut),
                MenuItem::os_action("Copy", text_input::Copy, OsAction::Copy),
                MenuItem::os_action("Paste", text_input::Paste, OsAction::Paste),
                MenuItem::os_action("Select All", text_input::SelectAll, OsAction::SelectAll),
            ],
            disabled: false,
        },
        Menu {
            name: "Task".into(),
            items: vec![
                MenuItem::action("New Task", chat::NewTask),
                MenuItem::action("Search Tasks", chat::FocusSearch),
                MenuItem::separator(),
                MenuItem::action("Next Task", chat::NextTask),
                MenuItem::action("Previous Task", chat::PreviousTask),
                MenuItem::separator(),
                MenuItem::action("Choose Project…", chat::ChooseProject),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Toggle Sidebar", chat::ToggleSidebar),
                MenuItem::action("Show Archived Tasks", chat::ToggleArchived),
            ],
            disabled: false,
        },
    ]);
}
