mod browsing;
mod clipboard;
mod dialogs;
mod navigation;
mod search;
mod shell;
mod sidebar;
mod transfers;

use std::{
    cell::Cell,
    collections::HashSet,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, atomic::AtomicU64},
    time::Duration,
};

use gio::prelude::*;

use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, Timer};

use crate::{AppWindow, ViewMode as UiViewMode, theme};

use super::state::{ExplorerState, ViewMode};
use super::tree::{mounted_roots, sync_tree};
use super::view::{clear_preview, sync_files, sync_navigation, sync_ranger_parent};
use super::{executor::TaskExecutor, settings};
use navigation::{DirectoryLoader, FsDirectoryLoader};

const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(75);
const DETAILS_DEBOUNCE: Duration = Duration::from_millis(50);
const RECURSIVE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(160);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MouseNavigation {
    Back,
    Forward,
}

struct Explorer {
    ui: slint::Weak<AppWindow>,
    state: Arc<Mutex<ExplorerState>>,
    volume_monitor: gio::VolumeMonitor,
    theme_settings: Option<gio::Settings>,
    preview_timer: Timer,
    details_timer: Timer,
    search_timer: Timer,
    search_generation: Arc<AtomicU64>,
    navigation_tasks: TaskExecutor,
    background_tasks: TaskExecutor,
    operation_tasks: TaskExecutor,
    directory_loader: Arc<dyn DirectoryLoader>,
    in_flight_navigation: Arc<Mutex<HashSet<PathBuf>>>,
}

fn to_ui_view_mode(mode: ViewMode) -> UiViewMode {
    match mode {
        ViewMode::Grid => UiViewMode::Grid,
        ViewMode::Ranger => UiViewMode::Ranger,
    }
}

fn from_ui_view_mode(mode: UiViewMode) -> ViewMode {
    match mode {
        UiViewMode::Grid => ViewMode::Grid,
        UiViewMode::Ranger => ViewMode::Ranger,
    }
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    slint::set_xdg_app_id("dev.polarexp.PolarExp")?;
    let explorer = Explorer::new(&ui);
    explorer.install_callbacks(&ui);
    explorer.start();
    ui.show()?;
    let event_ui = ui.as_weak();
    let window_event_task = slint::spawn_local(async move {
        let Some(ui) = event_ui.upgrade() else {
            return;
        };
        if ui.window().winit_window().await.is_ok() {
            install_window_event_handler(&ui);
        }
    })
    .map_err(|error| slint::PlatformError::Other(error.to_string()))?;
    let result = slint::run_event_loop();
    drop(window_event_task);
    ui.hide()?;
    result
}

fn install_window_event_handler(ui: &AppWindow) {
    let weak_ui = ui.as_weak();
    let modifiers = Cell::new(winit::keyboard::ModifiersState::default());
    let control_pressed = Cell::new(false);
    let cursor_position = Cell::new((0.0_f32, 0.0_f32));
    let drag_selection_anchor = Cell::new(None::<(i32, i32)>);
    ui.window().on_winit_window_event(move |window, event| {
        match event {
            winit::event::WindowEvent::ModifiersChanged(current) => {
                modifiers.set(current.state());
            }
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                if matches!(
                    event.physical_key,
                    winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::ControlLeft)
                        | winit::keyboard::PhysicalKey::Code(
                            winit::keyboard::KeyCode::ControlRight
                        )
                ) {
                    control_pressed.set(event.state == winit::event::ElementState::Pressed);
                    return EventResult::Propagate;
                }
                if event.state != winit::event::ElementState::Pressed
                    || (!modifiers.get().control_key() && !control_pressed.get())
                {
                    return EventResult::Propagate;
                }
                let Some(ui) = weak_ui.upgrade() else {
                    return EventResult::Propagate;
                };
                if ui.get_location_focused()
                    || ui.get_search_active()
                    || ui.get_busy()
                    || ui.get_dialog_kind() != crate::DialogKind::None
                {
                    return EventResult::Propagate;
                }
                match event.physical_key {
                    winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyC)
                        if ui.get_selected_entry() >= 0 =>
                    {
                        ui.invoke_copy_requested(ui.get_selected_entry());
                        return EventResult::PreventDefault;
                    }
                    winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyV) => {
                        ui.invoke_paste_requested();
                        return EventResult::PreventDefault;
                    }
                    _ => {}
                }
            }
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                let position = position.to_logical::<f32>(window.scale_factor().into());
                cursor_position.set((position.x, position.y));
                if let Some((start_row, start_column)) = drag_selection_anchor.get()
                    && let Some(ui) = weak_ui.upgrade()
                {
                    ui.set_drag_selection_current_x(position.x);
                    ui.set_drag_selection_current_y(position.y);
                    ui.invoke_drag_selection_requested(
                        start_row,
                        start_column,
                        ui.invoke_grid_selection_row_at(position.x, position.y),
                        ui.invoke_grid_selection_column_at(position.x, position.y),
                        ui.invoke_grid_selection_columns(),
                    );
                    return EventResult::PreventDefault;
                }
            }
            winit::event::WindowEvent::MouseInput { state, button, .. } => {
                if *button == winit::event::MouseButton::Left {
                    let Some(ui) = weak_ui.upgrade() else {
                        return EventResult::Propagate;
                    };
                    if *state == winit::event::ElementState::Pressed {
                        let (x, y) = cursor_position.get();
                        if ui.invoke_grid_selection_start_allowed(x, y) {
                            let row = ui.invoke_grid_selection_row_at(x, y);
                            let column = ui.invoke_grid_selection_column_at(x, y);
                            drag_selection_anchor.set(Some((row, column)));
                            ui.set_drag_selection_start_x(x);
                            ui.set_drag_selection_start_y(y);
                            ui.set_drag_selection_current_x(x);
                            ui.set_drag_selection_current_y(y);
                            ui.set_drag_selection_active(true);
                            ui.invoke_drag_selection_requested(
                                row,
                                column,
                                row,
                                column,
                                ui.invoke_grid_selection_columns(),
                            );
                            ui.invoke_focus_browser();
                            return EventResult::PreventDefault;
                        }
                    } else if drag_selection_anchor.take().is_some() {
                        ui.set_drag_selection_active(false);
                        ui.invoke_drag_selection_finished();
                        return EventResult::PreventDefault;
                    }
                }
                if !matches!(
                    button,
                    winit::event::MouseButton::Back | winit::event::MouseButton::Forward
                ) {
                    return EventResult::Propagate;
                }
                if let Some(navigation) = mouse_navigation_for_input(*state, *button)
                    && let Some(ui) = weak_ui.upgrade()
                {
                    dispatch_mouse_navigation(&ui, navigation);
                }
                return EventResult::PreventDefault;
            }
            winit::event::WindowEvent::Focused(true) => {
                if let Some(ui) = weak_ui.upgrade() {
                    ui.invoke_focus_browser();
                }
            }
            winit::event::WindowEvent::Focused(false) => {
                if drag_selection_anchor.take().is_some()
                    && let Some(ui) = weak_ui.upgrade()
                {
                    ui.set_drag_selection_active(false);
                    ui.invoke_drag_selection_finished();
                }
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

pub(super) fn mouse_navigation_for_input(
    state: winit::event::ElementState,
    button: winit::event::MouseButton,
) -> Option<MouseNavigation> {
    if state != winit::event::ElementState::Pressed {
        return None;
    }
    match button {
        winit::event::MouseButton::Back => Some(MouseNavigation::Back),
        winit::event::MouseButton::Forward => Some(MouseNavigation::Forward),
        _ => None,
    }
}

pub(super) fn dispatch_mouse_navigation(ui: &AppWindow, navigation: MouseNavigation) {
    if ui.get_busy() || ui.get_dialog_kind() != crate::DialogKind::None {
        return;
    }
    match navigation {
        MouseNavigation::Back if ui.get_can_go_back() => ui.invoke_back_requested(),
        MouseNavigation::Forward if ui.get_can_go_forward() => ui.invoke_forward_requested(),
        _ => {}
    }
}

impl Explorer {
    fn navigation_allowed(&self) -> bool {
        self.ui.upgrade().is_some_and(|ui| !ui.get_busy())
    }

    fn mutations_allowed(&self) -> bool {
        self.ui
            .upgrade()
            .is_some_and(|ui| !ui.get_busy() && !ui.get_navigation_loading())
            && !self.state.lock().unwrap().recursive_search_active
    }

    fn new(ui: &AppWindow) -> Rc<Self> {
        let volume_monitor = gio::VolumeMonitor::get();
        let mounts = mounted_roots(&volume_monitor);
        let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut state = ExplorerState::new(start, mounts);
        state.view_mode = settings::load_view_mode();
        Rc::new(Self {
            ui: ui.as_weak(),
            state: Arc::new(Mutex::new(state)),
            volume_monitor,
            theme_settings: theme::interface_settings(),
            preview_timer: Timer::default(),
            details_timer: Timer::default(),
            search_timer: Timer::default(),
            search_generation: Arc::new(AtomicU64::new(0)),
            navigation_tasks: TaskExecutor::new("navigation", 2),
            background_tasks: TaskExecutor::new("background", 2),
            operation_tasks: TaskExecutor::new("operations", 1),
            directory_loader: Arc::new(FsDirectoryLoader),
            in_flight_navigation: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    fn install_callbacks(self: &Rc<Self>, ui: &AppWindow) {
        let explorer = Rc::downgrade(self);

        macro_rules! bind {
            ($signal:ident, |$explorer:ident $(, $arg:ident)*| $body:expr => $fallback:expr) => {{
                let explorer = explorer.clone();
                ui.$signal(move |$($arg),*| {
                    if let Some($explorer) = explorer.upgrade() {
                        $body
                    } else {
                        $fallback
                    }
                });
            }};
            ($signal:ident, |$explorer:ident $(, $arg:ident)*| $body:expr) => {{
                let explorer = explorer.clone();
                ui.$signal(move |$($arg),*| {
                    if let Some($explorer) = explorer.upgrade() {
                        $body
                    }
                });
            }};
        }

        bind!(on_back_requested, |explorer| explorer.go_back());
        bind!(on_forward_requested, |explorer| explorer.go_forward());
        bind!(on_parent_requested, |explorer| explorer.go_parent());
        bind!(on_copy_requested, |explorer, index| explorer
            .copy_selected_entry(index));
        bind!(on_paste_requested, |explorer| explorer.paste_copied_entry());
        bind!(on_search_started, |explorer| explorer.begin_search());
        bind!(on_search_changed, |explorer, query| explorer
            .update_search(query.as_str()));
        bind!(on_search_submitted, |explorer| explorer.submit_search());
        bind!(on_search_cancelled, |explorer| explorer.cancel_search());
        bind!(on_search_repeat_requested, |explorer, reverse| explorer
            .repeat_search(reverse));
        bind!(on_recursive_search_mode_requested, |explorer, enabled| {
            explorer.set_recursive_search_mode(enabled)
        });
        bind!(on_command_submitted, |explorer, command| {
            explorer.run_shell_command(command.as_str())
        });
        bind!(on_view_mode_requested, |explorer, mode| {
            explorer.set_view_mode(from_ui_view_mode(mode))
        });
        bind!(on_location_submitted, |explorer, path| {
            explorer.submit_location(path.as_str())
        });
        bind!(on_entry_clicked, |explorer, index| explorer
            .entry_clicked(index));
        bind!(on_entry_double_clicked, |explorer, index| {
            explorer.entry_double_clicked(index)
        });
        bind!(on_entry_context_selected, |explorer, index| {
            explorer.select_entry(index)
        });
        bind!(on_visual_selection_toggled, |explorer| explorer
            .toggle_visual_selection());
        bind!(on_visual_selection_cancelled, |explorer| explorer
            .cancel_visual_selection());
        bind!(on_delete_selection_requested, |explorer| explorer
            .show_selected_trash_dialog());
        bind!(on_delete_operator_cancelled, |explorer| explorer
            .cancel_delete_operator());
        bind!(
            on_delete_operator_motion_requested,
            |explorer, motion, columns| {
                explorer.apply_delete_operator(motion.as_str(), columns)
            }
        );
        bind!(
            on_drag_selection_requested,
            |explorer, start_row, start_column, end_row, end_column, columns| {
                explorer.update_drag_selection(
                    start_row,
                    start_column,
                    end_row,
                    end_column,
                    columns,
                )
            }
        );
        bind!(on_drag_selection_finished, |explorer| explorer
            .finish_drag_selection());
        bind!(
            on_selection_move_requested,
            |explorer, horizontal, vertical, columns| {
                explorer.move_selection(horizontal, vertical, columns)
            }
        );
        bind!(on_selected_entry_activated, |explorer| {
            explorer.activate_selected_entry()
        });
        bind!(on_ranger_selection_requested, |explorer, index| {
            explorer.select_entry(index)
        });
        bind!(on_ranger_entry_activated, |explorer, index| {
            explorer.activate_entry(index)
        });
        bind!(on_ranger_selection_move_requested, |explorer, delta| {
            explorer.move_ranger_selection(delta)
        });
        bind!(on_ranger_left_requested, |explorer| explorer
            .ranger_go_parent());
        bind!(on_ranger_parent_activated, |explorer, index| {
            explorer.ranger_parent_activated(index)
        });
        bind!(on_tree_row_activated, |explorer, index| {
            explorer.tree_row_activated(index)
        });
        bind!(on_entry_drop_allowed, |explorer, data, index| explorer.entry_drop_allowed(&data, index) => false);
        bind!(on_entry_dropped, |explorer, data, index| explorer.drop_on_entry(data, index) => false);
        bind!(on_ranger_parent_drop_allowed, |explorer, data, index| explorer.ranger_parent_drop_allowed(&data, index) => false);
        bind!(on_ranger_parent_dropped, |explorer, data, index| explorer.drop_on_ranger_parent(data, index) => false);
        bind!(on_tree_drop_allowed, |explorer, data, index| explorer.tree_drop_allowed(&data, index) => false);
        bind!(on_tree_dropped, |explorer, data, index| explorer.drop_on_tree(data, index) => false);
        bind!(on_rename_requested, |explorer, index| explorer
            .show_rename_dialog(index));
        bind!(on_trash_requested, |explorer, index| explorer
            .show_trash_dialog(index));
        bind!(on_name_submitted, |explorer, name| explorer
            .submit_name(name.as_str()));
        bind!(on_dialog_cancelled, |explorer| explorer.cancel_dialog());
        bind!(on_dialog_confirmed, |explorer| explorer.confirm_dialog());
    }

    fn start(self: &Rc<Self>) {
        self.refresh_system_theme();
        self.watch_system_theme();
        self.watch_mounts();
        if let Some(ui) = self.ui.upgrade() {
            let mut state = self.state.lock().unwrap();
            sync_navigation(&ui, &state);
            sync_files(&ui, &state);
            sync_tree(&ui, &mut state);
            sync_ranger_parent(&ui, &state);
            ui.set_view_mode(to_ui_view_mode(state.view_mode));
            clear_preview(&ui);
        }

        let root = {
            let state = self.state.lock().unwrap();
            state.roots.first().map(|node| (node.id, node.path.clone()))
        };
        if let Some((id, path)) = root {
            Self::load_folder_children(
                self.background_tasks.clone(),
                self.state.clone(),
                self.ui.clone(),
                id,
                path,
            );
        }

        self.refresh(None);
    }

    fn watch_mounts(self: &Rc<Self>) {
        macro_rules! connect_refresh {
            ($signal:ident) => {{
                let explorer = Rc::downgrade(self);
                self.volume_monitor.$signal(move |_, _| {
                    if let Some(explorer) = explorer.upgrade() {
                        explorer.refresh_mounts();
                    }
                });
            }};
        }

        connect_refresh!(connect_mount_added);
        connect_refresh!(connect_mount_removed);
        connect_refresh!(connect_mount_changed);
    }

    fn refresh_system_theme(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if let Some(colors) = theme::load(self.theme_settings.as_ref()) {
            let accent = colors.accent;
            let selection_foreground = colors.selection_foreground.unwrap_or(accent);
            ui.invoke_apply_system_theme(
                true,
                slint::Brush::SolidColor(accent),
                colors.selection_foreground.is_some(),
                slint::Brush::SolidColor(selection_foreground),
            );
        } else {
            ui.invoke_apply_system_theme(
                false,
                slint::Brush::default(),
                false,
                slint::Brush::default(),
            );
        }
    }

    fn watch_system_theme(self: &Rc<Self>) {
        let Some(settings) = &self.theme_settings else {
            return;
        };
        let explorer = Rc::downgrade(self);
        settings.connect_changed(None, move |_, key| {
            if matches!(key, "gtk-theme" | "color-scheme" | "accent-color")
                && let Some(explorer) = explorer.upgrade()
            {
                explorer.refresh_system_theme();
            }
        });
    }
}
