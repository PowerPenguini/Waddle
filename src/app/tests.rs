use std::path::PathBuf;

use iced::{event, keyboard, mouse};

use super::{
    App, BrowserFocus, InputMode, Message, VirtualLocation, breadcrumb_segments,
    nearest_existing_ancestor,
};
use crate::app::file_operation::{
    Completion as FileOperationCompletion, View as FileOperationView,
};
use crate::app::grid::{
    CONTENT_GUTTER, LIST_VIEW_TOP_INSET, Motion, SIDEBAR_WIDTH, TILE_ROW_HEIGHT,
    TOOLBAR_DIVIDER_HEIGHT, TOOLBAR_HEIGHT,
};
use crate::app::navigation::NavigationSession;
use crate::app::state::{ExplorerState, MountRoot};
use crate::fs::FileEntry;
use crate::transfer::{Action as TransferAction, ClipboardImport};

fn entry(name: &str) -> FileEntry {
    FileEntry {
        path: PathBuf::from("/start").join(name),
        name: name.into(),
        directory: false,
    }
}

fn press(app: &mut App, value: &'static str) {
    let key = keyboard::Key::Character(value.into());
    let _ = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some(value));
}

#[test]
fn breadcrumbs_preserve_each_navigable_ancestor() {
    assert_eq!(
        breadcrumb_segments(std::path::Path::new("/home/mateusz/Projects")),
        [
            ("/".to_owned(), PathBuf::from("/")),
            ("home".to_owned(), PathBuf::from("/home")),
            ("mateusz".to_owned(), PathBuf::from("/home/mateusz")),
            (
                "Projects".to_owned(),
                PathBuf::from("/home/mateusz/Projects"),
            ),
        ]
    );
}

#[test]
fn composite_focus_order_wraps_and_context_menu_traps_then_restores_it() {
    assert_eq!(BrowserFocus::Toolbar.moved(false), BrowserFocus::Location);
    assert_eq!(BrowserFocus::Location.moved(false), BrowserFocus::Sidebar);
    assert_eq!(BrowserFocus::Sidebar.moved(false), BrowserFocus::Entries);
    assert_eq!(BrowserFocus::Entries.moved(false), BrowserFocus::BottomBar);
    assert_eq!(BrowserFocus::BottomBar.moved(false), BrowserFocus::Toolbar);
    assert_eq!(BrowserFocus::Toolbar.moved(true), BrowserFocus::BottomBar);

    let (mut app, _) = App::new();
    app.browser_focus = BrowserFocus::Sidebar;
    app.context_menu = Some((0, iced::Point::ORIGIN));
    let tab = keyboard::Key::Named(keyboard::key::Named::Tab);
    let _ = app.handle_key(tab.clone(), tab, keyboard::Modifiers::empty(), None);
    assert_eq!(app.context_menu_cursor, 1);
    assert_eq!(app.browser_focus, BrowserFocus::Sidebar);

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(app.context_menu.is_none());
    assert_eq!(app.browser_focus, BrowserFocus::Sidebar);
}

#[test]
fn space_activates_the_focused_toolbar_control() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation_loading = false;
    app.browser_focus = BrowserFocus::Toolbar;
    app.toolbar_cursor = 4;
    let before = app
        .view_preferences
        .for_directory(app.navigation.current())
        .view;

    let space = keyboard::Key::Named(keyboard::key::Named::Space);
    let _ = app.handle_key(
        space.clone(),
        space,
        keyboard::Modifiers::empty(),
        Some(" "),
    );

    assert_ne!(
        app.view_preferences
            .for_directory(app.navigation.current())
            .view,
        before
    );
    assert_eq!(app.browser_focus, BrowserFocus::Toolbar);
}

#[test]
fn single_click_activation_keeps_modified_clicks_for_selection() {
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.replace_displayed_entries(vec![FileEntry {
        path: child.clone(),
        name: "child".into(),
        directory: true,
    }]);
    if !app.view_preferences.single_click_activation() {
        app.view_preferences.toggle_single_click_activation();
    }

    app.modifiers = keyboard::Modifiers::CTRL;
    let _ = app.activate_entry(0, false);
    assert!(app.navigation.pending_path().is_none());
    assert_eq!(app.grid.selected_entry(), Some(0));

    app.modifiers = keyboard::Modifiers::empty();
    let _ = app.activate_entry(0, false);
    assert_eq!(app.navigation.pending_path(), Some(child.as_path()));
}

#[test]
fn copy_message_uses_the_complete_visual_selection_in_display_order() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation.replace_displayed_entries(vec![
        entry("one.txt"),
        entry("two.txt"),
        entry("three.txt"),
    ]);
    app.grid.select_only(Some(0), 3);
    app.grid.toggle_visual_selection(3);
    app.grid.move_selection(Motion::Right, 3);
    app.grid.move_selection(Motion::Right, 3);

    let _ = app.update(Message::Copy);
    let request = app.transfers.paste(PathBuf::from("/target")).unwrap();

    assert_eq!(app.status, "Copied 3 items");
    assert_eq!(
        request.paths,
        [
            PathBuf::from("/start/one.txt"),
            PathBuf::from("/start/two.txt"),
            PathBuf::from("/start/three.txt"),
        ]
    );
}

#[test]
fn system_clipboard_completion_enters_the_same_transfer_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let source_directory = temp.path().join("external");
    let destination = temp.path().join("target");
    std::fs::create_dir(&source_directory).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(destination.clone());
    app.navigation_loading = false;
    let paths = vec![source_directory.join("one"), source_directory.join("two")];
    for path in &paths {
        std::fs::write(path, "x").unwrap();
    }

    let _ = app.update(Message::ClipboardRead(Ok(ClipboardImport {
        paths: paths.clone(),
        action: TransferAction::Copy,
        generation: None,
    })));

    let request = app.transfers.paste(destination).unwrap();
    assert_eq!(request.paths, paths);
    assert_eq!(request.action, TransferAction::Copy);
    assert!(app.transfer_queue.active_id().is_some());
}

#[test]
fn counted_browser_sequences_drive_grid_and_focused_sidebar_with_feedback() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation.replace_displayed_entries(
        (0..20)
            .map(|index| entry(&format!("{index}.txt")))
            .collect(),
    );
    app.grid.select_only(Some(0), 20);

    press(&mut app, "3");
    assert_eq!(app.status, "3  •  awaiting motion or operator");
    press(&mut app, "j");
    assert_eq!(app.grid.selected_entry(), Some(15));
    press(&mut app, "g");
    assert_eq!(app.status, "g  •  awaiting g");
    press(&mut app, "g");
    assert_eq!(app.grid.selected_entry(), Some(0));

    let child_id = app.explorer.allocate_node_id();
    app.explorer.roots[0].loading = false;
    app.explorer.roots[0].loaded = true;
    app.explorer.roots[0]
        .children
        .push(crate::app::state::FolderNode::folder(
            child_id,
            PathBuf::from("/tmp"),
        ));
    app.browser_focus = BrowserFocus::Sidebar;
    app.sidebar_cursor = Some(app.explorer.roots[0].id);

    press(&mut app, "j");
    assert_eq!(app.sidebar_cursor, Some(child_id));
    press(&mut app, "g");
    press(&mut app, "g");
    assert_eq!(app.sidebar_cursor, Some(app.explorer.roots[0].id));
    let last_sidebar_id = crate::app::tree::flatten_rows(&app.explorer, app.navigation.current())
        .last()
        .unwrap()
        .id;
    press(&mut app, "G");
    assert_eq!(app.sidebar_cursor, Some(last_sidebar_id));

    press(&mut app, "3");
    press(&mut app, "q");
    assert_eq!(app.status, "Invalid Browser sequence: 3q");
}

#[test]
fn popular_file_formats_use_distinct_icons() {
    use super::EntryIconKind;

    for (name, expected) in [
        ("main.rs", EntryIconKind::Code),
        ("Dockerfile", EntryIconKind::Code),
        ("manual.pdf", EntryIconKind::Pdf),
        ("notes.md", EntryIconKind::Document),
        ("photo.JPG", EntryIconKind::Image),
        ("song.flac", EntryIconKind::Audio),
        ("movie.mkv", EntryIconKind::Video),
        ("backup.tar.gz", EntryIconKind::Archive),
        ("budget.xlsx", EntryIconKind::Spreadsheet),
        ("slides.pptx", EntryIconKind::Presentation),
        ("unknown.bin", EntryIconKind::Generic),
    ] {
        assert_eq!(super::entry_icon_kind(&entry(name)), expected, "{name}");
    }

    let mut folder = entry("archive.zip");
    folder.directory = true;
    assert_eq!(super::entry_icon_kind(&folder), EntryIconKind::Folder);
}

#[test]
fn trash_location_shows_original_path_and_requires_permanent_delete_confirmation() {
    let (mut app, _) = App::new();
    let trashed = PathBuf::from("/tmp/Trash/files/report.txt.2");
    let original = PathBuf::from("/home/user/report.txt");
    let file = crate::fs::FileEntry {
        path: trashed.clone(),
        name: "report.txt".into(),
        directory: false,
    };
    app.virtual_location = Some(VirtualLocation::Trash);
    app.trash_entries = vec![crate::app::trash::Entry {
        file: file.clone(),
        receipt: crate::journal::TrashReceipt {
            original: original.clone(),
            trashed,
            info: PathBuf::from("/tmp/Trash/info/report.txt.2.trashinfo"),
        },
    }];
    app.navigation.replace_displayed_entries(vec![file]);
    app.grid.select_only(Some(0), 1);

    app.refresh_status();
    assert!(app.status.contains(&original.display().to_string()));
    let _ = app.update(Message::ContextDeletePermanent);
    assert!(matches!(
        app.file_operations.view(),
        crate::app::file_operation::View::PermanentDelete { detail, .. }
            if detail.contains("cannot be undone")
    ));
}

#[test]
fn iced_browser_modes_follow_the_command_prefixes() {
    let (mut app, _) = App::new();

    press(&mut app, "/");
    assert_eq!(app.browser_input.mode(), InputMode::Search);
    app.browser_input.leave_mode();

    press(&mut app, "!");
    assert_eq!(app.browser_input.mode(), InputMode::Command);
    assert_eq!(app.command.prefix(), Some('!'));
    app.browser_input.leave_mode();

    press(&mut app, ":");
    assert_eq!(app.browser_input.mode(), InputMode::Command);
    assert_eq!(app.command.prefix(), Some(':'));
}

#[test]
fn r_opens_inline_rename_for_the_active_entry() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt"), entry("two.txt")]);
    app.grid
        .select_only(Some(1), app.navigation.entries().len());

    press(&mut app, "r");

    assert_eq!(app.browser_input.mode(), InputMode::Rename);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Rename {
            value: "two.txt",
            error: ""
        }
    ));
}

#[test]
fn inline_rename_validates_and_escape_cancels() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    press(&mut app, "r");

    let _ = app.update(Message::RenameChanged("bad/name".to_owned()));
    let _ = app.update(Message::RenameSubmitted);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Rename { error, .. }
            if error == "The name cannot contain a slash or NUL character."
    ));
    assert!(!app.busy);

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
}

#[test]
fn successful_inline_rename_returns_to_the_browser() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    press(&mut app, "r");
    app.busy = true;

    let _ = app.finish_file_operation(FileOperationCompletion::Name {
        kind: crate::app::file_operation::NameKind::Rename {
            source: PathBuf::from("/start/one.txt"),
        },
        result: Ok(PathBuf::from("/start/renamed.txt")),
    });

    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
}

#[test]
fn new_folder_uses_an_inline_prompt_with_validation_and_escape() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;

    let _ = app.show_new_folder();
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::NewFolder {
            value: "",
            error: ""
        }
    ));

    let _ = app.update(Message::PromptInputChanged("bad/name".to_owned()));
    let _ = app.update(Message::PromptSubmit);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::NewFolder { error, .. }
            if error == "The name cannot contain a slash or NUL character."
    ));

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
}

#[test]
fn errors_expand_in_the_bottom_bar_and_escape_closes_them() {
    let (mut app, _) = App::new();
    app.show_error("Could not open the selected item".to_owned());

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Error { .. }
    ));
    assert!(app.output_expansion.value());
    assert!(app.expanded_bar_height > super::STATUS_HEIGHT);

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
    assert!(!app.output_expansion.value());
}

#[test]
fn trash_failure_uses_an_expanded_permanent_delete_prompt() {
    let (mut app, _) = App::new();
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    let _ = app.show_trash_prompt();
    app.busy = true;

    let _ = app.finish_file_operation(FileOperationCompletion::Trash(
        super::file_operation::TrashCompletion {
            failures: vec![(entry("one.txt"), "Trash is unavailable".to_owned())],
            receipts: Vec::new(),
        },
    ));

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::PermanentDelete { message, detail }
            if message.contains("Permanently delete")
            && detail.contains("Trash is unavailable")
            && detail.contains("cannot be undone")
    ));
    assert!(app.output_expansion.value());

    let enter = keyboard::Key::Named(keyboard::key::Named::Enter);
    let _ = app.handle_key(enter.clone(), enter, keyboard::Modifiers::empty(), None);
    assert!(app.busy);
}

#[test]
fn live_refresh_preserves_scroll_selection_rename_and_pending_cut_by_path() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    let current = app.navigation.current().to_path_buf();
    let first = FileEntry {
        path: current.join("first"),
        name: "first".into(),
        directory: false,
    };
    let cut = FileEntry {
        path: current.join("cut"),
        name: "cut".into(),
        directory: false,
    };
    app.navigation
        .replace_displayed_entries(vec![first.clone(), cut.clone()]);
    app.grid.select_only(Some(0), 2);
    app.grid.set_scroll(173.0);
    app.browser_input.enter(InputMode::Rename);
    app.transfers.cut(std::slice::from_ref(&cut));
    let request = app.navigation.refresh_selected(vec![first.path.clone()]);
    let requested = request.requested().to_path_buf();

    let _ = app.finish_navigation(requested, Ok((current, vec![first.clone(), cut])));

    assert_eq!(app.grid.scroll_offset(), 173.0);
    assert_eq!(app.browser_input.mode(), InputMode::Rename);
    assert_eq!(app.transfers.pending_cut_paths().len(), 1);
    assert_eq!(
        app.grid
            .selected_entry()
            .and_then(|index| app.navigation.entries().get(index))
            .map(|entry| &entry.path),
        Some(&first.path)
    );
}

#[test]
fn missing_directory_falls_back_to_the_nearest_existing_ancestor() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("gone/child");
    assert_eq!(nearest_existing_ancestor(&missing), temp.path());
}

#[test]
fn deletion_prompt_accepts_y_and_n_from_the_keyboard() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());

    let _ = app.show_trash_prompt();
    press(&mut app, "n");
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));

    let _ = app.show_trash_prompt();
    press(&mut app, "Y");
    assert!(app.busy);
    assert!(app.file_operations.is_busy());
}

#[test]
fn command_output_expands_and_collapses_through_the_animation_state() {
    let (mut app, _) = App::new();

    app.command.begin(':');
    app.command.change("help".to_owned());
    let _ = app.command.submit(PathBuf::from("/work"));
    app.sync_bottom_bar();
    assert!(app.command.output().is_some());
    assert!(app.output_expansion.value());
    assert!(app.expanded_bar_height > super::STATUS_HEIGHT);

    app.command.close_output();
    app.sync_bottom_bar();
    assert!(app.command.output().is_none());
    assert!(!app.output_expansion.value());
}

#[test]
fn captured_escape_leaves_the_recursive_search_input() {
    let (mut app, _) = App::new();
    app.browser_input.enter(InputMode::Search);
    app.search.begin(&app.grid);
    let _ = app
        .search
        .update(&mut app.navigation, &mut app.grid, "/needle".to_owned());
    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: escape.clone(),
            modified_key: escape,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        }),
        event::Status::Captured,
    );

    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(!app.search.is_active());
    assert!(app.search.query().is_empty());
}

#[test]
fn mouse_side_buttons_request_back_and_forward_navigation() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current"));
    app.navigation.seed_history(
        vec![PathBuf::from("/back")],
        vec![PathBuf::from("/forward")],
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    assert_eq!(
        app.navigation.pending_path(),
        Some(PathBuf::from("/back").as_path())
    );

    app.navigation_loading = false;
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)),
        event::Status::Captured,
    );
    assert_eq!(
        app.navigation.pending_path(),
        Some(PathBuf::from("/forward").as_path())
    );
}

#[test]
fn iced_vim_keys_toggle_visual_mode_and_arm_cut_operator() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());

    press(&mut app, "v");
    assert!(app.grid.visual_active());

    press(&mut app, "v");
    assert!(!app.grid.visual_active());
    press(&mut app, "d");
    assert!(app.delete_operator_pending());

    press(&mut app, "$");
    assert_eq!(
        app.transfers.pending_cut_paths(),
        [PathBuf::from("/start/one"), PathBuf::from("/start/two")]
    );
    assert!(app.navigation.entries().is_empty());
    assert_eq!(app.status, "Cut: 2 items, p paste, Esc cancel");
}

#[test]
fn dd_cuts_only_the_active_item() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two"), entry("three")]);
    app.grid
        .select_only(Some(1), app.navigation.entries().len());

    press(&mut app, "d");
    assert!(app.delete_operator_pending());
    press(&mut app, "d");

    assert_eq!(
        app.transfers.pending_cut_paths(),
        [PathBuf::from("/start/two")]
    );
    assert_eq!(
        app.navigation
            .entries()
            .iter()
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["one", "three"]
    );
}

#[test]
fn black_hole_delete_trashes_without_replacing_the_clipboard() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid.select_only(Some(0), 2);
    press(&mut app, "y");
    let copied = app.transfers.clipboard_payload().unwrap();

    app.grid.select_only(Some(1), 2);
    for key in ["\"", "_", "d", "d"] {
        press(&mut app, key);
    }

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Trash { message } if message.contains("two")
    ));
    assert_eq!(app.transfers.clipboard_payload(), Some(copied));
}

#[test]
fn visual_cut_hides_the_complete_selection_and_escape_restores_it() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two"), entry("three")]);
    app.grid.select_only(Some(0), 3);
    app.grid.toggle_visual_selection(3);
    app.grid.move_selection(Motion::Right, 3);
    app.grid.move_selection(Motion::Right, 3);

    press(&mut app, "x");

    assert_eq!(app.transfers.pending_cut_paths().len(), 3);
    assert!(app.navigation.entries().is_empty());

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);

    assert!(app.transfers.pending_cut_paths().is_empty());
    assert_eq!(
        app.navigation.pending_path(),
        Some(app.navigation.current())
    );
}

#[test]
fn directory_events_confirm_only_observed_external_cut_moves() {
    let temp = tempfile::tempdir().unwrap();
    let source_directory = temp.path().join("source");
    let destination_directory = temp.path().join("destination");
    std::fs::create_dir(&source_directory).unwrap();
    std::fs::create_dir(&destination_directory).unwrap();
    let source = source_directory.join("item");
    std::fs::write(&source, "x").unwrap();
    let (mut app, _) = App::new();
    app.transfers
        .cut(&[FileEntry {
            path: source.clone(),
            name: "item".into(),
            directory: false,
        }])
        .unwrap();

    std::fs::rename(&source, destination_directory.join("item")).unwrap();
    let _ = app.update(Message::DirectoryChanged(super::directory_watch::Event {
        path: source_directory.clone(),
        moved_out: Vec::new(),
    }));
    assert_eq!(
        app.transfers.pending_cut_paths(),
        std::slice::from_ref(&source)
    );

    let _ = app.update(Message::DirectoryChanged(super::directory_watch::Event {
        path: source_directory,
        moved_out: vec![source],
    }));
    assert!(app.transfers.pending_cut_paths().is_empty());
    assert!(
        app.status_notice
            .as_deref()
            .unwrap()
            .contains("External move confirmed")
    );
}

#[test]
fn focused_sidebar_does_not_apply_file_operators_to_the_grid() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation.replace_displayed_entries(vec![entry("one")]);
    app.grid.select_only(Some(0), 1);
    app.browser_focus = BrowserFocus::Sidebar;
    app.sidebar_cursor = Some(app.explorer.roots[0].id);

    press(&mut app, "d");

    assert!(app.transfers.pending_cut_paths().is_empty());
    assert!(app.status.contains("sidebar"));
}

#[test]
fn standalone_row_edge_motions_move_the_active_selection() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation.replace_displayed_entries(
        (0..8)
            .map(|index| entry(&format!("entry-{index}")))
            .collect(),
    );
    app.grid
        .select_only(Some(6), app.navigation.entries().len());

    press(&mut app, "0");
    assert_eq!(app.grid.selected_entry(), Some(5));
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [5]
    );

    press(&mut app, "$");
    assert_eq!(app.grid.selected_entry(), Some(7));
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [7]
    );
}

#[test]
fn captured_browser_key_still_enters_visual_selection() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    let key = keyboard::Key::Character("v".into());

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyV),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: Some("v".into()),
            repeat: false,
        }),
        event::Status::Captured,
    );

    assert!(app.grid.visual_active());
}

#[test]
fn raw_mouse_drag_selects_a_grid_rectangle_and_finishes_on_release() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation.replace_displayed_entries(
        (0..9)
            .map(|index| entry(&format!("item-{index}")))
            .collect(),
    );
    let content_top =
        TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + CONTENT_GUTTER + LIST_VIEW_TOP_INSET;
    let start = iced::Point::new(SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0, content_top + 110.0);
    let end = iced::Point::new(
        SIDEBAR_WIDTH + CONTENT_GUTTER + app.grid.column_width() + 2.0,
        content_top + TILE_ROW_HEIGHT + 30.0,
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved { position: start }),
        event::Status::Ignored,
    );
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Ignored,
    );
    assert!(app.grid.marquee_bounds(app.status_height()).is_some());

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved { position: end }),
        event::Status::Ignored,
    );
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [0, 1, 5, 6]
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        event::Status::Ignored,
    );
    assert!(app.grid.marquee_bounds(app.status_height()).is_none());
}

#[test]
fn entry_drag_activates_after_six_pixels_and_selects_the_grabbed_item() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    app.grid.move_cursor(
        iced::Point::new(100.0, 100.0),
        app.navigation.entries().len(),
    );

    let _ = app.update(Message::EntryPressed(1));
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(105.0, 100.0),
        }),
        event::Status::Ignored,
    );
    assert!(app.transfers.active_drag_index().is_none());

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(106.0, 100.0),
        }),
        event::Status::Ignored,
    );
    assert_eq!(app.transfers.active_drag_index(), Some(1));
    assert_eq!(app.grid.selected_entry(), Some(1));
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn internal_drag_preview_only_appears_after_the_drag_threshold() {
    let (mut app, _) = App::new();
    app.navigation.replace_displayed_entries(vec![entry("one")]);
    app.transfers.press(0, iced::Point::ORIGIN, 1);
    assert!(app.drag_preview_view().is_none());

    app.transfers.move_pointer(iced::Point::new(6.0, 0.0));
    app.transfers
        .capture_drag_entries(app.navigation.entries(), app.grid.selected_indices());
    assert!(app.drag_preview_view().is_some());
}

#[test]
fn incoming_drop_targets_folders_empty_grid_and_rejects_files_and_toolbar() {
    let (mut app, _) = App::new();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation = NavigationSession::new(PathBuf::from("/start"));
    let mut folder = entry("folder");
    folder.directory = true;
    app.navigation
        .replace_displayed_entries(vec![folder.clone(), entry("file.txt")]);

    assert_eq!(
        app.drop_destination_at(iced::Point::new(250.0, 80.0), true),
        Some(folder.path)
    );
    assert_eq!(
        app.drop_destination_at(iced::Point::new(365.0, 80.0), true),
        None
    );
    assert_eq!(
        app.drop_destination_at(iced::Point::new(790.0, 400.0), true),
        Some(PathBuf::from("/start"))
    );
    assert_eq!(
        app.drop_destination_at(iced::Point::new(300.0, 20.0), true),
        None
    );
}

#[test]
fn drag_notice_survives_refresh_and_clears_on_the_next_interaction() {
    let (mut app, _) = App::new();
    app.status_notice = Some("Drop failed".to_owned());
    app.refresh_status();
    assert_eq!(app.status_notice.as_deref(), Some("Drop failed"));

    let _ = app.update(Message::Event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Ignored,
    ));
    assert!(app.status_notice.is_none());
}

#[test]
fn entering_an_earlier_tile_is_not_cleared_by_the_previous_tiles_exit() {
    let (mut app, _) = App::new();
    app.grid.enter(1);

    let _ = app.update(Message::EntryHovered(0));
    let _ = app.update(Message::EntryUnhovered(1));

    assert_eq!(app.grid.hovered(), Some(0));
}

#[test]
fn sidebar_tree_hover_and_selection_remain_translucent() {
    let (app, _) = App::new();
    let theme = app.iced_theme();
    let hover = super::tree_button_style(
        &theme,
        iced::widget::button::Status::Hovered,
        false,
        false,
        false,
    );
    let selected = super::tree_button_style(
        &theme,
        iced::widget::button::Status::Active,
        true,
        false,
        false,
    );

    assert!(matches!(
        hover.background,
        Some(iced::Background::Color(color)) if color.a == 0.06
    ));
    assert!(matches!(
        selected.background,
        Some(iced::Background::Color(color)) if color.a == 0.22
    ));
}

#[test]
fn spinner_animation_runs_for_tree_and_background_loading() {
    let (mut app, _) = App::new();
    assert!(app.spinner_active());

    app.explorer.roots[0].loading = false;
    app.navigation_loading = false;
    assert!(!app.spinner_active());

    app.busy = true;
    assert!(app.spinner_active());
}

#[test]
fn mount_reconciliation_preserves_existing_nodes() {
    let first = MountRoot {
        path: PathBuf::from("/media/first"),
        label: "First".to_owned(),
    };
    let mut state = ExplorerState::new(vec![first.clone()]);
    let original_id = state.roots[1].id;
    state.roots[1].expanded = true;

    let second = MountRoot {
        path: PathBuf::from("/media/second"),
        label: "Second".to_owned(),
    };
    assert!(state.reconcile_mounts(vec![first, second]));
    assert_eq!(state.roots[1].id, original_id);
    assert!(state.roots[1].expanded);
    assert_eq!(state.roots[2].label, "Second");
}
