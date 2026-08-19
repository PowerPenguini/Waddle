use std::path::PathBuf;

use iced::{event, keyboard, mouse};

use super::{App, DialogState, InputMode};
use crate::app::settings::parse_view_mode;
use crate::app::state::{ExplorerState, MountRoot, NavigationKind, PendingNavigation, ViewMode};
use crate::fs::FileEntry;

fn state() -> ExplorerState {
    ExplorerState::new(PathBuf::from("/start"), Vec::new())
}

fn entry(name: &str) -> FileEntry {
    FileEntry {
        path: PathBuf::from("/start").join(name),
        name: name.into(),
        directory: false,
    }
}

fn directory(path: &str) -> FileEntry {
    let path = PathBuf::from(path);
    FileEntry {
        name: path.file_name().unwrap_or_default().into(),
        path,
        directory: true,
    }
}

fn navigation(path: &str, kind: NavigationKind) -> PendingNavigation {
    PendingNavigation {
        requested: PathBuf::from(path),
        kind,
        select: None,
    }
}

fn press(app: &mut App, value: &'static str) {
    let key = keyboard::Key::Character(value.into());
    let _ = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some(value));
}

#[test]
fn iced_browser_modes_follow_the_command_prefixes() {
    let (mut app, _) = App::new();

    press(&mut app, "/");
    assert_eq!(app.input_mode, InputMode::Search);
    app.input_mode = InputMode::Browser;

    press(&mut app, "!");
    assert_eq!(app.input_mode, InputMode::Command('!'));
    app.input_mode = InputMode::Browser;

    press(&mut app, ":");
    assert_eq!(app.input_mode, InputMode::Command(':'));
}

#[test]
fn captured_escape_leaves_the_recursive_search_input() {
    let (mut app, _) = App::new();
    app.input_mode = InputMode::Search;
    app.explorer.recursive_search_active = true;
    app.search_text = "needle".to_owned();
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

    assert_eq!(app.input_mode, InputMode::Browser);
    assert!(!app.explorer.recursive_search_active);
    assert!(app.search_text.is_empty());
}

#[test]
fn mouse_side_buttons_request_back_and_forward_navigation() {
    let (mut app, _) = App::new();
    app.explorer.current = PathBuf::from("/current");
    app.explorer.history = vec![PathBuf::from("/back")];
    app.explorer.forward_history = vec![PathBuf::from("/forward")];

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    assert_eq!(
        app.explorer
            .pending_navigation
            .as_ref()
            .map(|navigation| navigation.requested.as_path()),
        Some(PathBuf::from("/back").as_path())
    );

    app.navigation_loading = false;
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)),
        event::Status::Captured,
    );
    assert_eq!(
        app.explorer
            .pending_navigation
            .as_ref()
            .map(|navigation| navigation.requested.as_path()),
        Some(PathBuf::from("/forward").as_path())
    );
}

#[test]
fn ranger_drag_targets_parent_and_current_directory_rows() {
    let (mut app, _) = App::new();
    app.explorer.view_mode = ViewMode::Ranger;
    app.window_size.width = 800.0;
    app.explorer.parent_entries = vec![directory("/parent")];
    app.explorer.entries = vec![directory("/current/child")];

    app.cursor = iced::Point::new(50.0, super::TOOLBAR_HEIGHT + 15.0);
    assert_eq!(
        app.drop_destination(PathBuf::from("/source/item").as_path()),
        Some(PathBuf::from("/parent"))
    );

    app.cursor = iced::Point::new(300.0, super::TOOLBAR_HEIGHT + 15.0);
    assert_eq!(
        app.drop_destination(PathBuf::from("/source/item").as_path()),
        Some(PathBuf::from("/current/child"))
    );
}

#[test]
fn iced_vim_keys_toggle_visual_mode_and_arm_delete_operator() {
    let (mut app, _) = App::new();
    app.navigation_loading = false;
    app.explorer.entries = vec![entry("one"), entry("two")];
    app.explorer.select_only(Some(0));

    press(&mut app, "v");
    assert_eq!(app.explorer.visual_selection_anchor, Some(0));

    press(&mut app, "v");
    assert_eq!(app.explorer.visual_selection_anchor, None);
    press(&mut app, "d");
    assert!(app.delete_operator_pending());

    press(&mut app, "$");
    assert!(matches!(app.dialog, DialogState::Trash { .. }));
    assert_eq!(
        app.explorer
            .selected_entries
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn successful_navigation_commits_location_and_history() {
    let mut state = state();
    let pending = navigation("/next", NavigationKind::Forward { remember: true });
    state.begin_navigation(pending.clone());
    assert_eq!(state.current, PathBuf::from("/start"));
    assert!(state.commit_navigation(pending, PathBuf::from("/next"), Vec::new()));
    assert_eq!(state.history, [PathBuf::from("/start")]);
    assert_eq!(state.current, PathBuf::from("/next"));
}

#[test]
fn regular_navigation_clears_forward_history() {
    let mut state = state();
    state.forward_history.push(PathBuf::from("/abandoned"));
    let pending = navigation("/next", NavigationKind::Forward { remember: true });

    assert!(state.commit_navigation(pending, PathBuf::from("/next"), Vec::new()));
    assert!(state.forward_history.is_empty());
}

#[test]
fn navigating_to_current_path_does_not_duplicate_history() {
    let mut state = state();
    let pending = navigation("/start", NavigationKind::Forward { remember: true });
    assert!(state.commit_navigation(pending, PathBuf::from("/start"), Vec::new()));
    assert!(state.history.is_empty());
}

#[test]
fn vim_selection_starts_at_the_nearest_edge() {
    let mut state = state();
    state.entries = vec![entry("one"), entry("two"), entry("three")];

    assert_eq!(state.move_selection(1, 0, 2), Some(0));
    state.selected_entry = None;
    assert_eq!(state.move_selection(0, -1, 2), Some(2));
}

#[test]
fn vim_selection_moves_horizontally_without_wrapping_rows() {
    let mut state = state();
    state.entries = (0..8).map(|index| entry(&index.to_string())).collect();
    state.selected_entry = Some(3);

    assert_eq!(state.move_selection(-1, 0, 3), Some(3));
    assert_eq!(state.move_selection(1, 0, 3), Some(4));
    assert_eq!(state.move_selection(1, 0, 3), Some(5));
    assert_eq!(state.move_selection(1, 0, 3), Some(5));
}

#[test]
fn vim_selection_moves_vertically_and_handles_a_short_last_row() {
    let mut state = state();
    state.entries = (0..8).map(|index| entry(&index.to_string())).collect();
    state.selected_entry = Some(1);

    assert_eq!(state.move_selection(0, 1, 3), Some(4));
    assert_eq!(state.move_selection(0, 1, 3), Some(7));
    assert_eq!(state.move_selection(0, 1, 3), Some(7));
    assert_eq!(state.move_selection(0, -1, 3), Some(4));

    state.selected_entry = Some(5);
    assert_eq!(state.move_selection(0, 1, 3), Some(7));
}

#[test]
fn vim_selection_ignores_an_empty_folder() {
    let mut state = state();
    state.selected_entry = Some(4);

    assert_eq!(state.move_selection(1, 0, 3), None);
    assert_eq!(state.selected_entry, None);
}

#[test]
fn visual_selection_extends_from_its_anchor_and_can_be_cancelled() {
    let mut state = state();
    state.entries = (0..8).map(|index| entry(&index.to_string())).collect();
    state.select_only(Some(1));

    state.toggle_visual_selection();
    assert_eq!(state.visual_selection_anchor, Some(1));
    assert_eq!(state.move_selection(0, 1, 3), Some(4));
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );

    state.cancel_visual_selection();
    assert_eq!(state.visual_selection_anchor, None);
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [4]
    );
}

#[test]
fn drag_selection_uses_a_rectangular_grid_range() {
    let mut state = state();
    state.entries = (0..8).map(|index| entry(&index.to_string())).collect();

    state.select_rectangle(0, 1, 2, 2, 3);

    assert_eq!(state.selected_entry, Some(7));
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [1, 2, 4, 5, 7]
    );

    state.select_rectangle(8, 0, 8, 0, 3);
    assert_eq!(state.selected_entry, None);
    assert!(state.selected_entries.is_empty());
}

#[test]
fn delete_operator_selects_vim_style_grid_motions() {
    let mut state = state();
    state.entries = (0..8).map(|index| entry(&index.to_string())).collect();
    state.select_only(Some(4));

    assert!(state.select_delete_motion("0", 3));
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [3, 4]
    );
    assert!(state.select_delete_motion("$", 3));
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [4, 5]
    );
    assert!(state.select_delete_motion("d", 3));
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [3, 4, 5]
    );
    assert!(state.select_delete_motion("j", 3));
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [3, 4, 5, 6, 7]
    );
    assert!(state.select_delete_motion("k", 3));
    assert_eq!(
        state.selected_entries.iter().copied().collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 5]
    );
    assert!(!state.select_delete_motion("w", 3));
}

#[test]
fn ranger_selection_moves_linearly_and_stops_at_edges() {
    let mut state = state();
    state.entries = vec![entry("one"), entry("two"), entry("three")];

    assert_eq!(state.move_ranger_selection(1), Some(0));
    assert_eq!(state.move_ranger_selection(1), Some(1));
    assert_eq!(state.move_ranger_selection(1), Some(2));
    assert_eq!(state.move_ranger_selection(1), Some(2));
    assert_eq!(state.move_ranger_selection(-1), Some(1));
}

#[test]
fn stale_ranger_previews_are_rejected() {
    let mut state = state();
    state.entries = vec![entry("one"), entry("two")];
    state.selected_entry = Some(0);
    let first = state.begin_preview();
    let first_path = state.entries[0].path.clone();
    assert!(state.accepts_preview(first, &first_path));

    state.selected_entry = Some(1);
    let second = state.begin_preview();
    assert!(!state.accepts_preview(first, &first_path));
    assert!(state.accepts_preview(second, &state.entries[1].path));
}

#[test]
fn view_mode_parser_defaults_to_grid() {
    assert_eq!(parse_view_mode("view-mode=ranger\n"), ViewMode::Ranger);
    assert_eq!(parse_view_mode("view-mode=grid\n"), ViewMode::Grid);
    assert_eq!(parse_view_mode("view-mode=unknown\n"), ViewMode::Grid);
    assert_eq!(parse_view_mode(""), ViewMode::Grid);
}

#[test]
fn only_the_latest_navigation_is_accepted() {
    let mut state = state();
    state.begin_navigation(navigation(
        "/first",
        NavigationKind::Forward { remember: true },
    ));
    state.begin_navigation(navigation(
        "/second",
        NavigationKind::Forward { remember: true },
    ));
    assert!(
        state
            .take_navigation_for(PathBuf::from("/first").as_path())
            .is_none()
    );
    assert!(
        state
            .take_navigation_for(PathBuf::from("/second").as_path())
            .is_some()
    );
}

#[test]
fn failed_navigation_never_changes_location_history_or_selection() {
    let mut state = state();
    state.selected_entry = Some(2);
    state.begin_navigation(navigation(
        "/unreadable",
        NavigationKind::Forward { remember: true },
    ));
    state.cancel_navigation();

    assert_eq!(state.current, PathBuf::from("/start"));
    assert_eq!(state.selected_entry, Some(2));
    assert!(state.history.is_empty());
}

#[test]
fn successful_back_navigation_pops_the_expected_history_entry() {
    let mut state = state();
    state.current = PathBuf::from("/current");
    state.history.push(PathBuf::from("/previous"));
    let pending = navigation(
        "/previous",
        NavigationKind::Back {
            expected: PathBuf::from("/previous"),
        },
    );
    assert!(state.commit_navigation(pending, PathBuf::from("/previous"), Vec::new()));
    assert_eq!(state.current, PathBuf::from("/previous"));
    assert!(state.history.is_empty());
    assert_eq!(state.forward_history, [PathBuf::from("/current")]);
}

#[test]
fn successful_forward_navigation_restores_the_newer_location() {
    let mut state = state();
    state.current = PathBuf::from("/previous");
    state.forward_history.push(PathBuf::from("/current"));
    let pending = navigation(
        "/current",
        NavigationKind::HistoryForward {
            expected: PathBuf::from("/current"),
        },
    );

    assert!(state.commit_navigation(pending, PathBuf::from("/current"), Vec::new()));
    assert_eq!(state.current, PathBuf::from("/current"));
    assert_eq!(state.history, [PathBuf::from("/previous")]);
    assert!(state.forward_history.is_empty());
}

#[test]
fn mount_reconciliation_preserves_existing_nodes() {
    let first = MountRoot {
        path: PathBuf::from("/media/first"),
        label: "First".to_owned(),
    };
    let mut state = ExplorerState::new(PathBuf::from("/"), vec![first.clone()]);
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
