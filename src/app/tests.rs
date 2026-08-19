use crate::AppWindow;
use crate::ViewMode as UiViewMode;
use crate::app::explorer::{
    MouseNavigation, dispatch_mouse_navigation, mouse_navigation_for_input,
};
use crate::app::settings::parse_view_mode;
use crate::app::state::{ExplorerState, MountRoot, NavigationKind, PendingNavigation, ViewMode};
use crate::fs::FileEntry;
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

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

fn navigation(path: &str, kind: NavigationKind) -> PendingNavigation {
    PendingNavigation {
        requested: PathBuf::from(path),
        kind,
        select: None,
    }
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
fn browser_keyboard_shortcuts_dispatch_expected_actions() {
    let ui = AppWindow::new().unwrap();
    let parent_invocations = Rc::new(Cell::new(0));
    let callback_invocations = parent_invocations.clone();
    ui.on_parent_requested(move || callback_invocations.set(callback_invocations.get() + 1));
    ui.set_can_go_parent(true);
    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Backspace.into(),
        });

    assert_eq!(parent_invocations.get(), 1);

    let back_invocations = Rc::new(Cell::new(0));
    let callback_invocations = back_invocations.clone();
    ui.on_back_requested(move || callback_invocations.set(callback_invocations.get() + 1));
    dispatch_mouse_navigation(&ui, MouseNavigation::Back);
    assert_eq!(back_invocations.get(), 0);
    ui.set_can_go_back(true);
    dispatch_mouse_navigation(&ui, MouseNavigation::Back);
    assert_eq!(back_invocations.get(), 1);

    let forward_invocations = Rc::new(Cell::new(0));
    let callback_invocations = forward_invocations.clone();
    ui.on_forward_requested(move || callback_invocations.set(callback_invocations.get() + 1));
    ui.set_can_go_forward(true);
    dispatch_mouse_navigation(&ui, MouseNavigation::Forward);
    assert_eq!(forward_invocations.get(), 1);

    ui.set_busy(true);
    dispatch_mouse_navigation(&ui, MouseNavigation::Back);
    dispatch_mouse_navigation(&ui, MouseNavigation::Forward);
    assert_eq!(back_invocations.get(), 1);
    assert_eq!(forward_invocations.get(), 1);
    ui.set_busy(false);
    ui.invoke_focus_browser();

    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "u".into() });

    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Control.into(),
        });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "o".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: "o".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::Control.into(),
        });

    assert_eq!(back_invocations.get(), 1);

    let deletion = Rc::new(Cell::new(0));
    let deletion_requests = deletion.clone();
    ui.on_delete_selection_requested(move || deletion_requests.set(deletion_requests.get() + 1));
    ui.set_selected_entry(2);
    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Delete.into(),
        });

    assert_eq!(deletion.get(), 1);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "x".into() });
    assert_eq!(deletion.get(), 2);

    let operator_motion = Rc::new(RefCell::new(String::new()));
    let captured_motion = operator_motion.clone();
    ui.on_delete_operator_motion_requested(move |motion, _| {
        *captured_motion.borrow_mut() = motion.to_string();
    });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "d".into() });
    assert!(ui.get_delete_operator_pending());
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "$".into() });
    assert!(!ui.get_delete_operator_pending());
    assert_eq!(operator_motion.borrow().as_str(), "$");

    let copied = Rc::new(Cell::new(-1));
    let copied_index = copied.clone();
    ui.on_copy_requested(move |index| copied_index.set(index));
    let pasted = Rc::new(Cell::new(0));
    let paste_invocations = pasted.clone();
    ui.on_paste_requested(move || paste_invocations.set(paste_invocations.get() + 1));
    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "y".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "p".into() });
    assert_eq!(copied.get(), 2);
    assert_eq!(pasted.get(), 1);

    copied.set(-1);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Control.into(),
        });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "c".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: "c".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "v".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: "v".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::Control.into(),
        });
    assert_eq!(copied.get(), 2);
    assert_eq!(pasted.get(), 2);

    let delta = Rc::new(Cell::new(0));
    let callback_delta = delta.clone();
    ui.on_ranger_selection_move_requested(move |value| callback_delta.set(value));
    ui.set_view_mode(UiViewMode::Ranger);
    ui.set_navigation_loading(true);
    ui.invoke_focus_browser();

    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "j".into() });

    assert_eq!(delta.get(), 1);
    ui.set_navigation_loading(false);

    let started = Rc::new(Cell::new(0));
    let started_callback = started.clone();
    ui.on_search_started(move || started_callback.set(started_callback.get() + 1));
    let changed = Rc::new(RefCell::new(String::new()));
    let changed_callback = changed.clone();
    ui.on_search_changed(move |query| {
        *changed_callback.borrow_mut() = query.to_string();
    });
    let submitted = Rc::new(Cell::new(0));
    let submitted_callback = submitted.clone();
    ui.on_search_submitted(move || submitted_callback.set(submitted_callback.get() + 1));
    let cancelled = Rc::new(Cell::new(0));
    let cancelled_callback = cancelled.clone();
    ui.on_search_cancelled(move || cancelled_callback.set(cancelled_callback.get() + 1));
    let repeat_direction = Rc::new(RefCell::new(Vec::new()));
    let repeat_callback = repeat_direction.clone();
    ui.on_search_repeat_requested(move |reverse| {
        repeat_callback.borrow_mut().push(reverse);
    });

    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "/".into() });
    assert!(ui.get_search_active());
    assert_eq!(started.get(), 1);

    // Raw TextInput editing is exercised by the live UI. The headless Slint
    // test backend does not route synthetic text events into that primitive.
    ui.set_search_text("a".into());
    ui.invoke_search_changed("a".into());
    assert_eq!(changed.borrow().as_str(), "a");
    ui.set_search_active(false);
    ui.invoke_search_submitted();
    assert!(!ui.get_search_active());
    assert_eq!(submitted.get(), 1);

    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "/".into() });
    ui.set_search_active(false);
    ui.set_search_text("".into());
    ui.invoke_search_cancelled();
    assert!(!ui.get_search_active());
    assert_eq!(cancelled.get(), 1);

    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "n".into() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "N".into() });
    assert_eq!(repeat_direction.borrow().as_slice(), [false, true]);

    ui.invoke_focus_browser();
    ui.set_navigation_loading(true);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "!".into() });
    assert!(!ui.get_command_active());
    ui.set_navigation_loading(false);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "!".into() });
    assert!(ui.get_command_active());
    // As with the search field above, the headless backend does not route
    // synthetic key events into the focused TextInput primitive.
    ui.set_command_text("pwd".into());
    ui.set_command_active(false);
    ui.set_command_text("".into());
    assert!(!ui.get_command_active());
    assert!(ui.get_command_text().is_empty());

    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: ":".into() });
    assert!(ui.get_command_active());
    assert_eq!(ui.get_command_prefix().as_str(), ":");
    ui.set_command_active(false);

    let visual_toggles = Rc::new(Cell::new(0));
    let callback_toggles = visual_toggles.clone();
    ui.on_visual_selection_toggled(move || callback_toggles.set(callback_toggles.get() + 1));
    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "v".into() });
    assert_eq!(visual_toggles.get(), 1);

    let visual_cancellations = Rc::new(Cell::new(0));
    let callback_cancellations = visual_cancellations.clone();
    ui.on_visual_selection_cancelled(move || {
        callback_cancellations.set(callback_cancellations.get() + 1)
    });
    ui.set_visual_selection_active(true);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        });
    assert_eq!(visual_cancellations.get(), 1);
    ui.set_visual_selection_active(false);

    ui.set_command_output_active(true);
    ui.invoke_focus_browser();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        });
    assert!(!ui.get_command_output_active());
}

#[test]
fn mouse_navigation_buttons_map_pressed_events_only() {
    use slint::winit_030::winit::event::{ElementState, MouseButton};

    assert_eq!(
        mouse_navigation_for_input(ElementState::Pressed, MouseButton::Back),
        Some(MouseNavigation::Back)
    );
    assert_eq!(
        mouse_navigation_for_input(ElementState::Pressed, MouseButton::Forward),
        Some(MouseNavigation::Forward)
    );
    assert_eq!(
        mouse_navigation_for_input(ElementState::Released, MouseButton::Back),
        None
    );
    assert_eq!(
        mouse_navigation_for_input(ElementState::Pressed, MouseButton::Left),
        None
    );
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
