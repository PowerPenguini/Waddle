use std::collections::BTreeSet;

use iced::{Point, Rectangle, Size, time::Instant};

pub(super) use super::drag_hover::{Effect as DragHoverEffect, Target as DragHoverTarget};
use super::{
    SCROLLBAR_FADE_IN, SCROLLBAR_FADE_OUT, SCROLLBAR_HOLD, SCROLLBAR_TRACK_WIDTH, drag_hover,
    duration_ratio, scroll_motion,
};

pub(super) const SIDEBAR_WIDTH: f32 = 220.0;
pub(super) const TOOLBAR_HEIGHT: f32 = 46.0;
pub(super) const TOOLBAR_DIVIDER_HEIGHT: f32 = 1.0;
pub(super) const TILE_WIDTH: f32 = 104.0;
pub(super) const TILE_HEIGHT: f32 = 108.0;
pub(super) const TILE_ROW_HEIGHT: f32 = 116.0;
pub(super) const CONTENT_GUTTER: f32 = 14.0;
pub(super) const LIST_VIEW_TOP_INSET: f32 = 6.0;
pub(super) const LIST_HEADER_HEIGHT: f32 = 26.0;
pub(super) const LIST_ROW_HEIGHT: f32 = 34.0;

const TILE_PITCH: f32 = 112.0;
const TREE_TOP: f32 = 44.0;
const TREE_ROW_HEIGHT: f32 = 32.0;
const AUTOSCROLL_EDGE: f32 = 44.0;
const AUTOSCROLL_STEP: f32 = 12.0;
const MARQUEE_DRAG_THRESHOLD: f32 = 6.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DropZone {
    Sidebar(usize),
    Entry(usize),
    Current,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScrollTarget {
    Sidebar,
    Entries,
}

#[derive(Clone, Debug, Default)]
struct ScrollbarVisibility {
    shown_at: Option<Instant>,
    fade_out_at: Option<Instant>,
}

impl ScrollbarVisibility {
    fn show(&mut self, now: Instant) {
        self.shown_at.get_or_insert(now);
        self.fade_out_at = Some(now + SCROLLBAR_HOLD);
    }

    fn hide_if_elapsed(&mut self, now: Instant) {
        if self
            .fade_out_at
            .is_some_and(|fade_out_at| now >= fade_out_at + SCROLLBAR_FADE_OUT)
        {
            self.shown_at = None;
            self.fade_out_at = None;
        }
    }

    fn is_visible(&self) -> bool {
        self.fade_out_at.is_some()
    }

    fn opacity(&self, now: Instant, reduced_motion: bool) -> f32 {
        let (Some(shown_at), Some(fade_out_at)) = (self.shown_at, self.fade_out_at) else {
            return 0.0;
        };
        if reduced_motion {
            return 1.0;
        }
        if now < fade_out_at {
            return duration_ratio(now.saturating_duration_since(shown_at), SCROLLBAR_FADE_IN);
        }

        1.0 - duration_ratio(
            now.saturating_duration_since(fade_out_at),
            SCROLLBAR_FADE_OUT,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ContextTarget {
    Background,
    Entry(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ContextMenu {
    pub(super) target: ContextTarget,
    pub(super) point: Point,
    pub(super) focused: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ContextNavigation {
    Previous { wrap: bool },
    Next { wrap: bool },
    First,
    Last,
    Activate,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ContextOutcome {
    None,
    Activate(usize),
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Motion {
    Left,
    Down,
    Up,
    Right,
    RowStart,
    RowEnd,
    First,
    Last,
    DisplayIndex(usize),
    ViewportTop,
    ViewportMiddle,
    ViewportBottom,
    HalfPageDown,
    HalfPageUp,
}

impl Motion {
    pub(super) fn is_directional(self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::Up | Self::Down)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeleteMotion {
    Current,
    Motion(Motion),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct VisibleRange {
    pub(super) columns: usize,
    pub(super) column_width: f32,
    pub(super) first_index: usize,
    pub(super) last_index: usize,
    pub(super) top_space: f32,
    pub(super) bottom_space: f32,
}

#[derive(Clone, Debug)]
struct Marquee {
    start: Point,
    start_scroll_y: f32,
    current: Point,
    dragging: bool,
}

impl Marquee {
    fn start_in_viewport(&self, scroll_y: f32) -> Point {
        Point::new(self.start.x, self.start.y + self.start_scroll_y - scroll_y)
    }

    fn move_to(&mut self, point: Point) {
        self.current = point;
        let x = point.x - self.start.x;
        let y = point.y - self.start.y;
        self.dragging |= x * x + y * y >= MARQUEE_DRAG_THRESHOLD * MARQUEE_DRAG_THRESHOLD;
    }
}

#[derive(Clone, Debug)]
pub(super) struct GridInteraction {
    window_size: Size,
    sidebar_width: f32,
    scroll_y: f32,
    sidebar_scroll_y: f32,
    cursor: Point,
    marquee: Option<Marquee>,
    hovered: Option<usize>,
    selected: Option<usize>,
    selection: BTreeSet<usize>,
    visual_anchor: Option<usize>,
    selection_anchor: Option<usize>,
    details: Option<String>,
    list_mode: bool,
    context_menu: Option<ContextMenu>,
    sidebar_scrollbar: ScrollbarVisibility,
    entry_scrollbar: ScrollbarVisibility,
    sidebar_scroll: scroll_motion::Motion,
    entry_scroll: scroll_motion::Motion,
    drag_hover: drag_hover::State,
}

impl Default for GridInteraction {
    fn default() -> Self {
        Self::new(Size::new(820.0, 560.0))
    }
}

impl GridInteraction {
    fn new(window_size: Size) -> Self {
        Self {
            window_size,
            sidebar_width: SIDEBAR_WIDTH,
            scroll_y: 0.0,
            sidebar_scroll_y: 0.0,
            cursor: Point::ORIGIN,
            marquee: None,
            hovered: None,
            selected: None,
            selection: BTreeSet::new(),
            visual_anchor: None,
            selection_anchor: None,
            details: None,
            list_mode: false,
            context_menu: None,
            sidebar_scrollbar: ScrollbarVisibility::default(),
            entry_scrollbar: ScrollbarVisibility::default(),
            sidebar_scroll: scroll_motion::Motion::default(),
            entry_scroll: scroll_motion::Motion::default(),
            drag_hover: drag_hover::State::default(),
        }
    }

    pub(super) fn observe_scroll(
        &mut self,
        target: ScrollTarget,
        offset: f32,
        maximum: f32,
        now: Instant,
        entry_count: usize,
    ) -> bool {
        match target {
            ScrollTarget::Sidebar => {
                let offset = offset.max(0.0);
                let moved = (self.sidebar_scroll_y - offset).abs() > f32::EPSILON;
                if moved {
                    self.sidebar_scrollbar.show(now);
                }
                self.sidebar_scroll_y = offset;
                self.sidebar_scroll.observe(offset, maximum);
                moved
            }
            ScrollTarget::Entries => {
                let offset = offset.max(0.0);
                let moved = (self.scroll_y - offset).abs() > f32::EPSILON;
                if moved {
                    self.entry_scrollbar.show(now);
                }
                self.scroll_y = offset;
                self.entry_scroll.observe(offset, maximum);
                if moved {
                    self.update_marquee_selection(entry_count);
                }
                moved
            }
        }
    }

    pub(super) fn wheel_scroll(
        &mut self,
        target: ScrollTarget,
        delta: iced::mouse::ScrollDelta,
        shift: bool,
        smooth: bool,
        now: Instant,
    ) -> Option<scroll_motion::Command> {
        self.scroll_mut(target).wheel(delta, shift, smooth, now)
    }

    pub(super) fn touchpad_scroll(
        &mut self,
        target: ScrollTarget,
        delta: iced::mouse::ScrollDelta,
        shift: bool,
        momentum: bool,
        now: Instant,
    ) {
        self.scroll_mut(target)
            .touchpad(delta, shift, momentum, now);
    }

    pub(super) fn scroll_to(
        &mut self,
        target: ScrollTarget,
        offset: f32,
        smooth: bool,
        now: Instant,
    ) -> Option<scroll_motion::Command> {
        self.scroll_mut(target).move_to(offset, smooth, now)
    }

    pub(super) fn tick_scroll(
        &mut self,
        now: Instant,
    ) -> Vec<(ScrollTarget, scroll_motion::Command)> {
        [ScrollTarget::Sidebar, ScrollTarget::Entries]
            .into_iter()
            .filter_map(|target| {
                self.scroll_mut(target)
                    .tick(now)
                    .map(|command| (target, command))
            })
            .collect()
    }

    pub(super) fn scroll_animation_active(&self) -> bool {
        self.sidebar_scroll.active() || self.entry_scroll.active()
    }

    pub(super) fn cancel_scroll(&mut self, target: ScrollTarget) {
        self.scroll_mut(target).cancel();
    }

    pub(super) fn cancel_scrolls(&mut self) {
        self.sidebar_scroll.cancel();
        self.entry_scroll.cancel();
    }

    fn scroll_mut(&mut self, target: ScrollTarget) -> &mut scroll_motion::Motion {
        match target {
            ScrollTarget::Sidebar => &mut self.sidebar_scroll,
            ScrollTarget::Entries => &mut self.entry_scroll,
        }
    }

    fn settle_scrollbars(&mut self, now: Instant) {
        self.sidebar_scrollbar.hide_if_elapsed(now);
        self.entry_scrollbar.hide_if_elapsed(now);
    }

    pub(super) fn scrollbar_visible(&self) -> bool {
        self.sidebar_scrollbar.is_visible() || self.entry_scrollbar.is_visible()
    }

    pub(super) fn scrollbar_opacity(
        &self,
        scrollbar: ScrollTarget,
        now: Instant,
        reduced_motion: bool,
    ) -> f32 {
        match scrollbar {
            ScrollTarget::Sidebar => self.sidebar_scrollbar.opacity(now, reduced_motion),
            ScrollTarget::Entries => self.entry_scrollbar.opacity(now, reduced_motion),
        }
    }

    pub(super) fn set_drag_hover(&mut self, target: Option<DragHoverTarget>, now: Instant) {
        self.drag_hover.set(target, now);
    }

    pub(super) fn cancel_drag_hover(&mut self) {
        self.drag_hover.cancel();
    }

    pub(super) fn tick(&mut self, now: Instant) -> Option<DragHoverEffect> {
        self.settle_scrollbars(now);
        self.drag_hover.tick(now)
    }

    pub(super) fn drag_hover_progress(&self, path: &std::path::Path, now: Instant) -> Option<f32> {
        self.drag_hover.progress(path, now)
    }

    pub(super) fn resize(&mut self, size: Size) {
        self.window_size = size;
    }

    pub(super) fn window_width(&self) -> f32 {
        self.window_size.width
    }

    pub(super) fn set_sidebar_visible(&mut self, visible: bool) {
        self.sidebar_width = if visible { SIDEBAR_WIDTH } else { 0.0 };
    }

    pub(super) fn sidebar_width(&self) -> f32 {
        self.sidebar_width
    }

    pub(super) fn set_list_mode(&mut self, list_mode: bool) {
        self.list_mode = list_mode;
    }

    pub(super) fn install_navigation(
        &mut self,
        selected: &[usize],
        entry_count: usize,
        list_mode: bool,
        reset_scroll: bool,
    ) {
        self.list_mode = list_mode;
        self.select_indices(selected, entry_count);
        self.details = None;
        self.entry_scroll.cancel();
        if reset_scroll {
            self.reset_scroll();
        }
    }

    pub(super) fn cursor(&self) -> Point {
        self.cursor
    }

    pub(super) fn context_menu(&self) -> Option<ContextMenu> {
        self.context_menu
    }

    pub(super) fn open_entry_context(&mut self, index: usize, entry_count: usize) -> bool {
        if index >= entry_count {
            return false;
        }
        if self.selection.contains(&index) {
            self.selected = Some(index);
        } else {
            self.select_only(Some(index), entry_count);
        }
        self.context_menu = Some(ContextMenu {
            target: ContextTarget::Entry(index),
            point: self.cursor,
            focused: 0,
        });
        true
    }

    pub(super) fn open_background_context(
        &mut self,
        entry_count: usize,
        status_height: f32,
    ) -> bool {
        if !self.selection_start_allowed(self.cursor, entry_count, status_height) {
            return false;
        }
        self.select_only(None, entry_count);
        self.context_menu = Some(ContextMenu {
            target: ContextTarget::Background,
            point: self.cursor,
            focused: 0,
        });
        true
    }

    pub(super) fn close_context(&mut self) {
        self.context_menu = None;
    }

    pub(super) fn focus_context(&mut self, index: usize, item_count: usize) {
        if index >= item_count {
            return;
        }
        if let Some(menu) = self.context_menu.as_mut() {
            menu.focused = index;
        }
    }

    pub(super) fn take_context_entry(&mut self) -> Option<usize> {
        match self.context_menu.take()?.target {
            ContextTarget::Entry(index) => Some(index),
            ContextTarget::Background => None,
        }
    }

    pub(super) fn navigate_context(
        &mut self,
        navigation: ContextNavigation,
        item_count: usize,
    ) -> ContextOutcome {
        let Some(menu) = self.context_menu.as_mut() else {
            return ContextOutcome::None;
        };
        if navigation == ContextNavigation::Close {
            self.context_menu = None;
            return ContextOutcome::Closed;
        }
        if item_count == 0 {
            return ContextOutcome::None;
        }
        let last = item_count - 1;
        match navigation {
            ContextNavigation::Previous { wrap: true } => {
                menu.focused = menu.focused.checked_sub(1).unwrap_or(last);
            }
            ContextNavigation::Previous { wrap: false } => {
                menu.focused = menu.focused.saturating_sub(1);
            }
            ContextNavigation::Next { wrap: true } => {
                menu.focused = (menu.focused + 1) % item_count;
            }
            ContextNavigation::Next { wrap: false } => {
                menu.focused = (menu.focused + 1).min(last);
            }
            ContextNavigation::First => menu.focused = 0,
            ContextNavigation::Last => menu.focused = last,
            ContextNavigation::Activate => {
                return ContextOutcome::Activate(menu.focused.min(last));
            }
            ContextNavigation::Close => unreachable!(),
        }
        ContextOutcome::None
    }

    pub(super) fn move_cursor(&mut self, position: Point, entry_count: usize) -> bool {
        self.cursor = position;
        self.hovered = self
            .index_at(position, entry_count)
            .filter(|&index| self.point_over_entry(position, index));
        if let Some(marquee) = &mut self.marquee {
            marquee.move_to(position);
            self.update_marquee_selection(entry_count);
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(super) fn set_scroll(&mut self, y: f32) {
        self.scroll_y = y;
    }

    fn reset_scroll(&mut self) {
        self.scroll_y = 0.0;
        self.entry_scroll = scroll_motion::Motion::default();
    }

    #[cfg(test)]
    pub(super) fn scroll_offset(&self) -> f32 {
        self.scroll_y
    }

    pub(super) fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    pub(super) fn selected_entry(&self) -> Option<usize> {
        self.selected
    }

    pub(super) fn selected_indices(&self) -> &BTreeSet<usize> {
        &self.selection
    }

    pub(super) fn selection_count(&self) -> usize {
        self.selection.len()
    }

    pub(super) fn is_selected(&self, index: usize) -> bool {
        self.selection.contains(&index)
    }

    pub(super) fn visual_active(&self) -> bool {
        self.visual_anchor.is_some()
    }

    pub(super) fn details(&self) -> Option<&str> {
        self.details.as_deref()
    }

    pub(super) fn clear_details(&mut self) {
        self.details = None;
    }

    pub(super) fn set_details(&mut self, details: Option<String>) {
        self.details = details;
    }

    pub(super) fn select_only(&mut self, selected: Option<usize>, entry_count: usize) {
        self.selected = selected.filter(|index| *index < entry_count);
        self.selection.clear();
        self.selection.extend(self.selected);
        self.visual_anchor = None;
        self.selection_anchor = self.selected;
    }

    fn select_indices(&mut self, indices: &[usize], entry_count: usize) {
        self.selection = indices
            .iter()
            .copied()
            .filter(|index| *index < entry_count)
            .collect();
        self.selected = self.selection.first().copied();
        self.visual_anchor = None;
        self.selection_anchor = self.selected;
    }

    pub(super) fn select_click(
        &mut self,
        index: usize,
        control: bool,
        shift: bool,
        entry_count: usize,
    ) {
        if index >= entry_count {
            return;
        }
        if shift {
            let anchor = self.selection_anchor.or(self.selected).unwrap_or(index);
            self.selection.clear();
            self.selection.extend(anchor.min(index)..=anchor.max(index));
            self.selected = Some(index);
        } else if control {
            if !self.selection.remove(&index) {
                self.selection.insert(index);
            }
            self.selected = Some(index);
            self.selection_anchor = Some(index);
        } else {
            self.select_only(Some(index), entry_count);
        }
        self.visual_anchor = None;
    }

    pub(super) fn select_all(&mut self, entry_count: usize) {
        self.selection = (0..entry_count).collect();
        self.selected = (entry_count > 0).then_some(0);
        self.selection_anchor = self.selected;
        self.visual_anchor = None;
    }

    pub(super) fn toggle_active(&mut self, entry_count: usize) {
        let Some(active) = self.selected.filter(|index| *index < entry_count) else {
            return;
        };
        if !self.selection.remove(&active) {
            self.selection.insert(active);
        }
        self.selection_anchor.get_or_insert(active);
    }

    pub(super) fn move_standard(
        &mut self,
        motion: Motion,
        extend: bool,
        entry_count: usize,
        status_height: f32,
    ) -> Option<usize> {
        if entry_count == 0 {
            return None;
        }
        let current = self.selected.unwrap_or(0).min(entry_count - 1);
        let next = self.motion_target(current, motion, 1, entry_count, status_height);
        let anchor = self.selection_anchor.unwrap_or(current);
        self.selected = Some(next);
        if extend {
            self.selection.clear();
            self.selection.extend(anchor.min(next)..=anchor.max(next));
        } else {
            self.selection.clear();
            self.selection.insert(next);
            self.selection_anchor = Some(next);
        }
        self.visual_anchor = None;
        Some(next)
    }

    #[cfg(test)]
    pub(super) fn move_selection(&mut self, motion: Motion, entry_count: usize) -> Option<usize> {
        self.move_selection_count(motion, 1, entry_count, 25.0)
    }

    pub(super) fn move_selection_count(
        &mut self,
        motion: Motion,
        count: usize,
        entry_count: usize,
        status_height: f32,
    ) -> Option<usize> {
        if entry_count == 0 {
            self.select_only(None, entry_count);
            return None;
        }

        let last = entry_count - 1;
        let Some(current) = self.selected else {
            let next = match motion {
                Motion::Left
                | Motion::Up
                | Motion::RowEnd
                | Motion::Last
                | Motion::ViewportBottom
                | Motion::HalfPageUp => last,
                Motion::DisplayIndex(index) => index.min(last),
                Motion::Down
                | Motion::Right
                | Motion::RowStart
                | Motion::First
                | Motion::ViewportTop
                | Motion::ViewportMiddle
                | Motion::HalfPageDown => 0,
            };
            self.selected = Some(next);
            self.update_keyboard_selection();
            return self.selected;
        };

        let next = self.motion_target(current, motion, count.max(1), entry_count, status_height);
        self.selected = Some(next);
        self.update_keyboard_selection();
        self.selected
    }

    fn motion_target(
        &self,
        current: usize,
        motion: Motion,
        count: usize,
        entry_count: usize,
        status_height: f32,
    ) -> usize {
        let last = entry_count.saturating_sub(1);
        let columns = self.columns();
        match motion {
            Motion::Left => current.saturating_sub(count.min(current % columns)),
            Motion::Right => current
                .saturating_add(count.min(columns - 1 - current % columns))
                .min(last),
            Motion::Up => current.saturating_sub(columns.saturating_mul(count)),
            Motion::Down => current
                .saturating_add(columns.saturating_mul(count))
                .min(last),
            Motion::RowStart => current / columns * columns,
            Motion::RowEnd => (current / columns * columns)
                .saturating_add(columns - 1)
                .min(last),
            Motion::First => 0,
            Motion::Last => last,
            Motion::DisplayIndex(index) => index.min(last),
            Motion::ViewportTop | Motion::ViewportMiddle | Motion::ViewportBottom => {
                let visible = self.visible_range(entry_count, status_height);
                let anchor = match motion {
                    Motion::ViewportTop => visible.first_index,
                    Motion::ViewportMiddle => {
                        (visible.first_index + visible.last_index.saturating_sub(1)) / 2
                    }
                    Motion::ViewportBottom => visible.last_index.saturating_sub(1),
                    _ => unreachable!(),
                };
                anchor.min(last)
            }
            Motion::HalfPageDown | Motion::HalfPageUp => {
                let visible = self.visible_range(entry_count, status_height);
                let half_page = visible
                    .last_index
                    .saturating_sub(visible.first_index)
                    .div_ceil(2);
                let distance = half_page.max(1).saturating_mul(count);
                if motion == Motion::HalfPageDown {
                    current.saturating_add(distance).min(last)
                } else {
                    current.saturating_sub(distance)
                }
            }
        }
    }

    pub(super) fn toggle_visual_selection(&mut self, entry_count: usize) {
        if self.visual_anchor.take().is_some() {
            return;
        }
        if entry_count == 0 {
            self.select_only(None, entry_count);
            return;
        }
        let selected = self.selected.unwrap_or(0).min(entry_count - 1);
        self.selected = Some(selected);
        self.visual_anchor = Some(selected);
        self.update_keyboard_selection();
    }

    pub(super) fn cancel_visual_selection(&mut self, entry_count: usize) {
        self.select_only(self.selected, entry_count);
    }

    #[cfg(test)]
    pub(super) fn select_delete_motion(
        &mut self,
        motion: DeleteMotion,
        entry_count: usize,
    ) -> bool {
        self.select_delete_motion_count(motion, 1, entry_count, 25.0)
    }

    pub(super) fn select_delete_motion_count(
        &mut self,
        motion: DeleteMotion,
        count: usize,
        entry_count: usize,
        status_height: f32,
    ) -> bool {
        let Some(current) = self.selected.filter(|index| *index < entry_count) else {
            return false;
        };
        let last = entry_count - 1;
        let target = match motion {
            DeleteMotion::Current => current.saturating_add(count.saturating_sub(1)).min(last),
            DeleteMotion::Motion(motion) => {
                self.motion_target(current, motion, count.max(1), entry_count, status_height)
            }
        };
        self.selection.clear();
        self.selection
            .extend(current.min(target)..=current.max(target));
        self.visual_anchor = None;
        true
    }

    pub(super) fn start_marquee(
        &mut self,
        point: Point,
        entry_count: usize,
        status_height: f32,
        allowed: bool,
    ) -> bool {
        if !allowed || !self.selection_start_allowed(point, entry_count, status_height) {
            return false;
        }
        self.marquee = Some(Marquee {
            start: point,
            start_scroll_y: self.scroll_y,
            current: point,
            dragging: false,
        });
        self.update_marquee_selection(entry_count);
        true
    }

    pub(super) fn finish_marquee(&mut self) -> bool {
        self.marquee.take().is_some()
    }

    pub(super) fn marquee_drag_active(&self) -> bool {
        self.marquee
            .as_ref()
            .is_some_and(|marquee| marquee.dragging)
    }

    pub(super) fn marquee_bounds(&self, status_height: f32) -> Option<Rectangle> {
        let marquee = self.marquee.as_ref()?;
        let start = marquee.start_in_viewport(self.scroll_y);
        let origin = Point::new(self.sidebar_width, TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT);
        let size = Size::new(
            (self.window_size.width - origin.x).max(0.0),
            (self.window_size.height - origin.y - status_height).max(0.0),
        );
        let entries_top = (LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT).min(size.height);
        let left = (start.x.min(marquee.current.x) - origin.x).clamp(0.0, size.width);
        let right = (start.x.max(marquee.current.x) - origin.x).clamp(0.0, size.width);
        let top = (start.y.min(marquee.current.y) - origin.y).clamp(entries_top, size.height);
        let bottom = (start.y.max(marquee.current.y) - origin.y).clamp(entries_top, size.height);
        Some(Rectangle::new(
            Point::new(left, top),
            Size::new(right - left, bottom - top),
        ))
    }

    pub(super) fn marquee_top_clipped(&self) -> bool {
        let Some(marquee) = self.marquee.as_ref() else {
            return false;
        };
        let start = marquee.start_in_viewport(self.scroll_y);
        let origin_y = TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT;
        let top = start.y.min(marquee.current.y) - origin_y;
        top < LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT
    }

    pub(super) fn marquee_bottom_clipped(&self, status_height: f32) -> bool {
        let Some(marquee) = self.marquee.as_ref() else {
            return false;
        };
        let start = marquee.start_in_viewport(self.scroll_y);
        let origin_y = TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT;
        let bottom = start.y.max(marquee.current.y) - origin_y;
        let viewport_bottom = (self.window_size.height - origin_y - status_height).max(0.0);
        bottom > viewport_bottom
    }

    fn columns(&self) -> usize {
        if self.list_mode {
            return 1;
        }
        let width = (self.window_size.width - self.sidebar_width - 2.0 * CONTENT_GUTTER).max(1.0);
        (width / TILE_PITCH).floor().max(1.0) as usize
    }

    fn column_width(&self) -> f32 {
        let width = (self.window_size.width - self.sidebar_width - 2.0 * CONTENT_GUTTER).max(1.0);
        width / self.columns() as f32
    }

    pub(super) fn scroll_target(&self, index: usize) -> f32 {
        if self.list_mode {
            index as f32 * LIST_ROW_HEIGHT
        } else {
            (index / self.columns()) as f32 * TILE_ROW_HEIGHT
        }
    }

    pub(super) fn directional_scroll_target(&self, index: usize, status_height: f32) -> f32 {
        let row_height = if self.list_mode {
            LIST_ROW_HEIGHT
        } else {
            TILE_ROW_HEIGHT
        };
        let row = if self.list_mode {
            index
        } else {
            index / self.columns()
        };
        let viewport_height = (self.window_size.height
            - TOOLBAR_HEIGHT
            - TOOLBAR_DIVIDER_HEIGHT
            - status_height
            - LIST_VIEW_TOP_INSET
            - LIST_HEADER_HEIGHT)
            .max(row_height);
        let visible_rows = (viewport_height / row_height).ceil() as usize;
        let last_safe_row = visible_rows.saturating_sub(2);
        let selected_y = row as f32 * row_height;
        let safe_top = if self.scroll_y > f32::EPSILON && visible_rows > 2 {
            self.scroll_y + row_height
        } else {
            self.scroll_y
        };
        let safe_bottom = self.scroll_y + last_safe_row as f32 * row_height;

        if selected_y < safe_top {
            (selected_y - row_height).max(0.0)
        } else if selected_y > safe_bottom {
            selected_y - last_safe_row as f32 * row_height
        } else {
            self.scroll_y
        }
    }

    pub(super) fn visible_range(&self, entry_count: usize, status_height: f32) -> VisibleRange {
        let columns = self.columns();
        let total_rows = entry_count.div_ceil(columns);
        let viewport_height = self.window_size.height
            - TOOLBAR_HEIGHT
            - TOOLBAR_DIVIDER_HEIGHT
            - status_height
            - LIST_VIEW_TOP_INSET
            - LIST_HEADER_HEIGHT;
        let viewport_height = viewport_height.max(TILE_ROW_HEIGHT);
        let visible_rows = (viewport_height / TILE_ROW_HEIGHT).ceil() as usize + 2;
        let first_row = ((self.scroll_y / TILE_ROW_HEIGHT).floor() as usize)
            .saturating_sub(1)
            .min(total_rows.saturating_sub(visible_rows));
        let last_row = (first_row + visible_rows).min(total_rows);
        VisibleRange {
            columns,
            column_width: self.column_width(),
            first_index: first_row * columns,
            last_index: (last_row * columns).min(entry_count),
            top_space: first_row as f32 * TILE_ROW_HEIGHT,
            bottom_space: total_rows.saturating_sub(last_row) as f32 * TILE_ROW_HEIGHT,
        }
    }

    pub(super) fn list_visible_range(
        &self,
        entry_count: usize,
        status_height: f32,
    ) -> std::ops::Range<usize> {
        let viewport =
            (self.window_size.height - TOOLBAR_HEIGHT - status_height).max(LIST_ROW_HEIGHT);
        let count = (viewport / LIST_ROW_HEIGHT).ceil() as usize + 2;
        let first = ((self.scroll_y / LIST_ROW_HEIGHT).floor() as usize)
            .saturating_sub(1)
            .min(entry_count.saturating_sub(count));
        first..first.saturating_add(count).min(entry_count)
    }

    pub(super) fn drop_zone(
        &self,
        point: Point,
        entry_count: usize,
        tree_row_count: usize,
        status_height: f32,
        allow_current: bool,
    ) -> Option<DropZone> {
        if point.y < 0.0 || point.y >= self.window_size.height {
            return None;
        }
        if point.x < self.sidebar_width {
            let index =
                ((point.y - TREE_TOP + self.sidebar_scroll_y) / TREE_ROW_HEIGHT).floor() as isize;
            let index = usize::try_from(index).ok()?;
            return (index < tree_row_count).then_some(DropZone::Sidebar(index));
        }
        if point.x >= self.window_size.width
            || point.y < TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT
            || point.y >= self.window_size.height - status_height
        {
            return None;
        }
        if let Some(index) = self.index_at(point, entry_count)
            && self.point_over_entry(point, index)
        {
            return Some(DropZone::Entry(index));
        }
        allow_current.then_some(DropZone::Current)
    }

    pub(super) fn cursor_outside_window(&self) -> bool {
        self.cursor.x < 0.0
            || self.cursor.y < 0.0
            || self.cursor.x >= self.window_size.width
            || self.cursor.y >= self.window_size.height
    }

    pub(super) fn drag_preview_origin(&self) -> Point {
        Point::new(
            (self.cursor.x + 14.0).min((self.window_size.width - 62.0).max(0.0)),
            (self.cursor.y + 16.0).min((self.window_size.height - 62.0).max(0.0)),
        )
    }

    pub(super) fn drag_autoscroll(&self, status_height: f32) -> (f32, f32) {
        let top = TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT;
        let bottom = self.window_size.height - status_height;
        if self.cursor.x < self.sidebar_width {
            (
                0.0,
                edge_autoscroll_delta(self.cursor.y, 30.0, self.window_size.height),
            )
        } else {
            (edge_autoscroll_delta(self.cursor.y, top, bottom), 0.0)
        }
    }

    pub(super) fn marquee_autoscroll(&self, status_height: f32) -> f32 {
        if !self.marquee_drag_active()
            || self.cursor.x < self.sidebar_width
            || self.cursor.x >= self.window_size.width
        {
            return 0.0;
        }
        edge_autoscroll_delta(
            self.cursor.y,
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT,
            self.window_size.height - status_height,
        )
    }

    pub(super) fn selected_items<T: Clone>(&self, items: &[T]) -> Vec<T> {
        if self.selection.len() > 1 {
            self.selection
                .iter()
                .filter_map(|index| items.get(*index).cloned())
                .collect()
        } else {
            self.selected
                .and_then(|index| items.get(index).cloned())
                .into_iter()
                .collect()
        }
    }

    fn selection_start_allowed(
        &self,
        point: Point,
        entry_count: usize,
        status_height: f32,
    ) -> bool {
        let scrollbar_right = self.window_size.width - CONTENT_GUTTER;
        let scrollbar_left = scrollbar_right - SCROLLBAR_TRACK_WIDTH;
        let over_entry_scrollbar = point.x >= scrollbar_left && point.x < scrollbar_right;
        if point.x < self.sidebar_width
            || point.x >= self.window_size.width
            || over_entry_scrollbar
            || point.y < self.entries_top()
            || point.y >= self.window_size.height - status_height
        {
            return false;
        }
        let Some(index) = self.index_at(point, entry_count) else {
            return true;
        };
        !self.point_over_entry(point, index)
    }

    fn cell_at(&self, point: Point) -> Option<(i32, i32)> {
        if point.x < self.sidebar_width + CONTENT_GUTTER || point.y < self.entries_top() {
            return None;
        }
        let column =
            ((point.x - self.sidebar_width - CONTENT_GUTTER) / self.column_width()).floor() as i32;
        let row =
            ((point.y - self.entries_top() + self.scroll_y) / self.row_height()).floor() as i32;
        (row >= 0 && column >= 0 && column < self.columns() as i32).then_some((row, column))
    }

    fn selection_cell_at(&self, point: Point, scroll_y: f32) -> (i32, i32) {
        let column =
            ((point.x - self.sidebar_width - CONTENT_GUTTER) / self.column_width()).floor() as i32;
        let row = ((point.y - self.entries_top() + scroll_y) / self.row_height()).floor() as i32;
        (row, column)
    }

    fn index_at(&self, point: Point, entry_count: usize) -> Option<usize> {
        let (row, column) = self.cell_at(point)?;
        let index = row as usize * self.columns() + column as usize;
        (index < entry_count).then_some(index)
    }

    fn point_over_entry(&self, point: Point, index: usize) -> bool {
        let bounds = self.entry_bounds(index);
        point.x >= bounds.x
            && point.x < bounds.x + bounds.width
            && point.y >= bounds.y
            && point.y < bounds.y + bounds.height
    }

    fn entry_bounds(&self, index: usize) -> Rectangle {
        if self.list_mode {
            return Rectangle::new(
                Point::new(
                    self.sidebar_width + CONTENT_GUTTER,
                    self.entries_top() - self.scroll_y + index as f32 * LIST_ROW_HEIGHT,
                ),
                Size::new(
                    (self.window_size.width - self.sidebar_width - 2.0 * CONTENT_GUTTER).max(0.0),
                    LIST_ROW_HEIGHT,
                ),
            );
        }
        let columns = self.columns();
        let row = index / columns;
        let column = index % columns;
        let column_left = self.sidebar_width + CONTENT_GUTTER + column as f32 * self.column_width();
        Rectangle::new(
            Point::new(
                column_left + (self.column_width() - TILE_WIDTH) / 2.0,
                content_top() - self.scroll_y + row as f32 * TILE_ROW_HEIGHT,
            ),
            Size::new(TILE_WIDTH, TILE_HEIGHT),
        )
    }

    fn entries_top(&self) -> f32 {
        if self.list_mode {
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT
        } else {
            content_top()
        }
    }

    fn row_height(&self) -> f32 {
        if self.list_mode {
            LIST_ROW_HEIGHT
        } else {
            TILE_ROW_HEIGHT
        }
    }

    fn update_marquee_selection(&mut self, entry_count: usize) {
        let Some(marquee) = &self.marquee else {
            return;
        };
        let start = marquee.start_in_viewport(self.scroll_y);
        let (start_row, start_column) =
            self.selection_cell_at(marquee.start, marquee.start_scroll_y);
        let (end_row, end_column) = self.selection_cell_at(marquee.current, self.scroll_y);
        let selection_bounds = Rectangle::new(
            Point::new(
                start.x.min(marquee.current.x),
                start.y.min(marquee.current.y),
            ),
            Size::new(
                (marquee.current.x - start.x).abs(),
                (marquee.current.y - start.y).abs(),
            ),
        );
        self.select_rectangle(
            start_row,
            start_column,
            end_row,
            end_column,
            selection_bounds,
            entry_count,
        );
    }

    fn select_rectangle(
        &mut self,
        start_row: i32,
        start_column: i32,
        end_row: i32,
        end_column: i32,
        selection_bounds: Rectangle,
        entry_count: usize,
    ) {
        self.visual_anchor = None;
        self.selection.clear();
        if entry_count == 0 {
            self.selected = None;
            return;
        }

        let columns = self.columns();
        let last_row = (entry_count - 1) / columns;
        let first_row = usize::try_from(start_row.min(end_row).max(0)).unwrap_or(0);
        let final_row = usize::try_from(start_row.max(end_row).max(0))
            .unwrap_or(usize::MAX)
            .min(last_row);
        let first_column = usize::try_from(start_column.min(end_column).max(0)).unwrap_or(0);
        let final_column = usize::try_from(start_column.max(end_column).max(0))
            .unwrap_or(usize::MAX)
            .min(columns - 1);
        if first_row > final_row || first_column > final_column {
            self.selected = None;
            return;
        }
        for row in first_row..=final_row {
            for column in first_column..=final_column {
                let index = row * columns + column;
                if index < entry_count
                    && rectangles_intersect(selection_bounds, self.entry_bounds(index))
                {
                    self.selection.insert(index);
                }
            }
        }

        let target_row = usize::try_from(end_row.max(0)).unwrap_or(0).min(last_row);
        let target_column = usize::try_from(end_column.max(0))
            .unwrap_or(0)
            .min(columns - 1);
        let target = target_row * columns + target_column;
        self.selected = self
            .selection
            .iter()
            .copied()
            .min_by_key(|index| index.abs_diff(target));
    }

    fn update_keyboard_selection(&mut self) {
        let Some(selected) = self.selected else {
            self.selection.clear();
            return;
        };
        self.selection.clear();
        if let Some(anchor) = self.visual_anchor {
            self.selection
                .extend(anchor.min(selected)..=anchor.max(selected));
        } else {
            self.selection.insert(selected);
        }
    }
}

fn edge_autoscroll_delta(y: f32, top: f32, bottom: f32) -> f32 {
    if y < top + AUTOSCROLL_EDGE {
        -AUTOSCROLL_STEP * (1.0 - ((y - top).max(0.0) / AUTOSCROLL_EDGE))
    } else if y > bottom - AUTOSCROLL_EDGE {
        AUTOSCROLL_STEP * (1.0 - ((bottom - y).max(0.0) / AUTOSCROLL_EDGE))
    } else {
        0.0
    }
}

fn rectangles_intersect(left: Rectangle, right: Rectangle) -> bool {
    left.x < right.x + right.width
        && left.x + left.width > right.x
        && left.y < right.y + right.height
        && left.y + left.height > right.y
}

fn content_top() -> f32 {
    TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> GridInteraction {
        GridInteraction::new(Size::new(584.0, 560.0))
    }

    #[test]
    fn first_grid_row_starts_immediately_below_the_sort_header() {
        assert_eq!(
            content_top(),
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT
        );
    }

    #[test]
    fn only_arrow_and_hjkl_motions_are_directional() {
        for motion in [Motion::Left, Motion::Right, Motion::Up, Motion::Down] {
            assert!(motion.is_directional());
        }
        for motion in [
            Motion::First,
            Motion::Last,
            Motion::ViewportTop,
            Motion::HalfPageDown,
        ] {
            assert!(!motion.is_directional());
        }
    }

    #[test]
    fn directional_scroll_keeps_one_visible_row_below_the_selection() {
        let now = Instant::now();
        let mut grid = GridInteraction::default();
        let columns = grid.columns();

        assert_eq!(grid.directional_scroll_target(columns, 25.0), 0.0);
        assert_eq!(grid.directional_scroll_target(columns * 2, 25.0), 0.0);
        assert_eq!(
            grid.directional_scroll_target(columns * 3, 25.0),
            TILE_ROW_HEIGHT
        );

        grid.observe_scroll(ScrollTarget::Entries, TILE_ROW_HEIGHT, 1_000.0, now, 30);
        assert_eq!(
            grid.directional_scroll_target(columns * 2, 25.0),
            TILE_ROW_HEIGHT
        );
        assert_eq!(grid.directional_scroll_target(columns, 25.0), 0.0);
    }

    #[test]
    fn only_real_scroll_movement_starts_and_restarts_scrollbar_timing() {
        let now = Instant::now();
        let mut grid = grid();

        assert!(!grid.scrollbar_visible());
        assert_eq!(
            grid.scrollbar_opacity(ScrollTarget::Entries, now, false),
            0.0
        );
        assert!(!grid.observe_scroll(ScrollTarget::Entries, 0.0, 1_000.0, now, 0));
        assert!(!grid.scrollbar_visible());

        assert!(grid.observe_scroll(ScrollTarget::Entries, 60.0, 1_000.0, now, 0));
        assert!(grid.scrollbar_visible());
        assert_eq!(
            grid.scrollbar_opacity(ScrollTarget::Entries, now + SCROLLBAR_FADE_IN / 2, false),
            0.5
        );
        assert_eq!(
            grid.scrollbar_opacity(
                ScrollTarget::Entries,
                now + SCROLLBAR_HOLD + SCROLLBAR_FADE_OUT / 2,
                false
            ),
            0.5
        );
        let _ = grid.tick(now + SCROLLBAR_HOLD + SCROLLBAR_FADE_OUT);
        assert!(!grid.scrollbar_visible());

        assert!(!grid.observe_scroll(
            ScrollTarget::Entries,
            60.0,
            1_000.0,
            now + SCROLLBAR_HOLD + SCROLLBAR_FADE_OUT,
            0,
        ));
        assert!(!grid.scrollbar_visible());
    }

    #[test]
    fn keyboard_and_visual_selection_share_one_state() {
        let mut grid = grid();

        assert_eq!(grid.move_selection(Motion::Right, 8), Some(0));
        grid.select_only(Some(1), 8);
        grid.toggle_visual_selection(8);
        assert_eq!(grid.move_selection(Motion::Down, 8), Some(4));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );

        grid.cancel_visual_selection(8);
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [4]
        );
        assert!(!grid.visual_active());
    }

    #[test]
    fn list_mode_uses_one_column_and_bounds_large_directory_rendering() {
        let mut grid = grid();
        grid.set_list_mode(true);
        grid.set_scroll(3_400.0);

        assert_eq!(grid.columns(), 1);
        assert_eq!(grid.scroll_target(125), 125.0 * LIST_ROW_HEIGHT);
        let visible = grid.list_visible_range(10_000, 25.0);
        assert!(visible.start > 0);
        assert!(visible.len() < 30);
        assert!(visible.end < 10_000);
    }

    #[test]
    fn hidden_sidebar_reclaims_grid_and_pointer_geometry() {
        let mut grid = grid();
        assert_eq!(grid.columns(), 3);
        assert_eq!(
            grid.drop_zone(Point::new(10.0, 100.0), 8, 4, 25.0, true),
            Some(DropZone::Sidebar(1))
        );

        grid.set_sidebar_visible(false);
        assert_eq!(grid.sidebar_width(), 0.0);
        assert_eq!(grid.columns(), 4);
        assert_ne!(
            grid.drop_zone(Point::new(10.0, 100.0), 8, 4, 25.0, true),
            Some(DropZone::Sidebar(1))
        );

        let start = Point::new(CONTENT_GUTTER + 2.0, content_top() + 2.0);
        assert!(grid.start_marquee(start, 8, 25.0, true));
        assert_eq!(grid.marquee_bounds(25.0).unwrap().x, CONTENT_GUTTER + 2.0);
    }

    #[test]
    fn moving_between_grid_tiles_clears_hover() {
        let mut grid = grid();
        let column_width = grid.column_width();
        let first_tile_left = SIDEBAR_WIDTH + CONTENT_GUTTER + (column_width - TILE_WIDTH) / 2.0;
        let second_tile_left = first_tile_left + column_width;
        let gap_middle = (first_tile_left + TILE_WIDTH + second_tile_left) / 2.0;
        let tile_middle_y = content_top() + TILE_HEIGHT / 2.0;

        grid.move_cursor(
            Point::new(first_tile_left + TILE_WIDTH / 2.0, tile_middle_y),
            3,
        );
        assert_eq!(grid.hovered(), Some(0));
        grid.move_cursor(Point::new(gap_middle, tile_middle_y), 3);

        assert_eq!(grid.hovered(), None);
    }

    #[test]
    fn marquee_moving_only_through_grid_gaps_selects_nothing() {
        let mut grid = grid();
        let column_width = grid.column_width();
        let first_tile_left = SIDEBAR_WIDTH + CONTENT_GUTTER + (column_width - TILE_WIDTH) / 2.0;
        let gap_middle = first_tile_left + (TILE_WIDTH + column_width) / 2.0;
        let start = Point::new(gap_middle, content_top() + TILE_HEIGHT / 2.0);
        let end = Point::new(
            gap_middle,
            content_top() + 2.0 * TILE_ROW_HEIGHT + TILE_HEIGHT / 2.0,
        );

        assert!(grid.start_marquee(start, 9, 25.0, true));
        grid.move_cursor(end, 9);

        assert!(grid.selected_indices().is_empty());
        assert_eq!(grid.selected_entry(), None);
    }

    #[test]
    fn marquee_line_crossing_grid_tiles_selects_only_those_tiles() {
        let mut grid = grid();
        let tile_center_x = SIDEBAR_WIDTH
            + CONTENT_GUTTER
            + (grid.column_width() - TILE_WIDTH) / 2.0
            + TILE_WIDTH / 2.0;
        let start = Point::new(tile_center_x, content_top() + 3.0 * TILE_ROW_HEIGHT + 2.0);
        let end = Point::new(tile_center_x, content_top() + TILE_HEIGHT / 2.0);

        assert!(grid.start_marquee(start, 9, 25.0, true));
        grid.move_cursor(end, 9);

        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [0, 3, 6]
        );
    }

    #[test]
    fn conventional_click_keyboard_and_active_selection_are_distinct() {
        let mut grid = GridInteraction::new(Size::new(820.0, 560.0));
        grid.select_click(2, false, false, 12);
        grid.select_click(5, true, false, 12);
        assert_eq!(grid.selected_entry(), Some(5));
        assert_eq!(grid.selected_indices(), &BTreeSet::from([2, 5]));

        grid.select_click(8, false, true, 12);
        assert_eq!(grid.selected_entry(), Some(8));
        assert_eq!(grid.selected_indices(), &(5..=8).collect());

        grid.move_standard(Motion::Left, true, 12, 25.0);
        assert_eq!(grid.selected_entry(), Some(7));
        assert_eq!(grid.selected_indices(), &(5..=7).collect());
        grid.toggle_active(12);
        assert!(!grid.is_selected(7));
        assert_eq!(grid.selected_entry(), Some(7));

        grid.select_all(12);
        assert_eq!(grid.selection_count(), 12);
    }

    #[test]
    fn keyboard_selection_respects_rows_and_short_last_row() {
        let mut grid = grid();
        grid.select_only(Some(1), 8);

        assert_eq!(grid.move_selection(Motion::Down, 8), Some(4));
        assert_eq!(grid.move_selection(Motion::Down, 8), Some(7));
        assert_eq!(grid.move_selection(Motion::Down, 8), Some(7));
        grid.select_only(Some(5), 8);
        assert_eq!(grid.move_selection(Motion::Down, 8), Some(7));
    }

    #[test]
    fn keyboard_selection_starts_at_the_nearest_edge_and_does_not_wrap_rows() {
        let mut grid = grid();

        assert_eq!(grid.move_selection(Motion::Right, 8), Some(0));
        grid.select_only(None, 8);
        assert_eq!(grid.move_selection(Motion::Up, 8), Some(7));
        grid.select_only(Some(3), 8);
        assert_eq!(grid.move_selection(Motion::Left, 8), Some(3));
        assert_eq!(grid.move_selection(Motion::Right, 8), Some(4));

        assert_eq!(grid.move_selection(Motion::Right, 0), None);
        assert_eq!(grid.selected_entry(), None);
    }

    #[test]
    fn row_edge_motions_respect_full_and_partial_rows() {
        let mut grid = grid();
        grid.select_only(Some(4), 8);

        assert_eq!(grid.move_selection(Motion::RowStart, 8), Some(3));
        assert_eq!(grid.move_selection(Motion::RowEnd, 8), Some(5));
        grid.select_only(Some(7), 8);
        assert_eq!(grid.move_selection(Motion::RowStart, 8), Some(6));
        assert_eq!(grid.move_selection(Motion::RowEnd, 8), Some(7));

        grid.select_only(None, 8);
        assert_eq!(grid.move_selection(Motion::RowStart, 8), Some(0));
        grid.select_only(None, 8);
        assert_eq!(grid.move_selection(Motion::RowEnd, 8), Some(7));
        assert_eq!(grid.move_selection(Motion::RowStart, 0), None);
    }

    #[test]
    fn row_edge_motions_extend_visual_selection_from_its_anchor() {
        let mut grid = grid();
        grid.select_only(Some(4), 8);
        grid.toggle_visual_selection(8);

        assert_eq!(grid.move_selection(Motion::RowStart, 8), Some(3));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [3, 4]
        );
        assert_eq!(grid.move_selection(Motion::RowEnd, 8), Some(5));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [4, 5]
        );
    }

    #[test]
    fn counted_absolute_and_viewport_motions_use_display_order() {
        let mut grid = grid();
        grid.select_only(Some(1), 60);

        assert_eq!(
            grid.move_selection_count(Motion::Down, 2, 60, 25.0),
            Some(7)
        );
        assert_eq!(
            grid.move_selection_count(Motion::DisplayIndex(12), 1, 60, 25.0),
            Some(12)
        );
        assert_eq!(
            grid.move_selection_count(Motion::First, 1, 60, 25.0),
            Some(0)
        );
        assert_eq!(
            grid.move_selection_count(Motion::Last, 1, 60, 25.0),
            Some(59)
        );

        grid.set_scroll(TILE_ROW_HEIGHT * 2.0);
        assert_eq!(
            grid.move_selection_count(Motion::ViewportTop, 1, 60, 25.0),
            Some(3)
        );
        assert_eq!(
            grid.move_selection_count(Motion::ViewportMiddle, 1, 60, 25.0),
            Some(11)
        );
        assert_eq!(
            grid.move_selection_count(Motion::ViewportBottom, 1, 60, 25.0),
            Some(20)
        );
        grid.select_only(Some(10), 60);
        assert_eq!(
            grid.move_selection_count(Motion::HalfPageDown, 1, 60, 25.0),
            Some(19)
        );
    }

    #[test]
    fn delete_motion_uses_the_same_grid_columns() {
        let mut grid = grid();
        grid.select_only(Some(4), 8);

        assert!(grid.select_delete_motion(DeleteMotion::Motion(Motion::RowStart), 8));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [3, 4]
        );
        assert!(grid.select_delete_motion(DeleteMotion::Motion(Motion::RowEnd), 8));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [4, 5]
        );
        assert!(grid.select_delete_motion(DeleteMotion::Current, 8));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [4]
        );
        assert!(grid.select_delete_motion(DeleteMotion::Motion(Motion::Down), 8));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [4, 5, 6, 7]
        );
        grid.select_only(None, 8);
        assert!(!grid.select_delete_motion(DeleteMotion::Current, 8));
    }

    #[test]
    fn marquee_clips_to_the_browser_and_maps_scrolled_cells() {
        let mut grid = GridInteraction::default();
        grid.set_scroll(TILE_ROW_HEIGHT);
        let start = Point::new(SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0, content_top() + 2.0);
        assert!(grid.start_marquee(start, 20, 25.0, true));
        grid.move_cursor(Point::new(400.0, 600.0), 20);

        let bounds = grid.marquee_bounds(25.0).unwrap();
        assert_eq!(bounds.x, CONTENT_GUTTER + 2.0);
        assert_eq!(
            bounds.y,
            content_top() + 2.0 - TOOLBAR_HEIGHT - TOOLBAR_DIVIDER_HEIGHT
        );
        assert_eq!(bounds.width, 164.0);
        assert_eq!(bounds.height, grid.window_size.height - 25.0 - start.y);
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [5, 6, 10, 11, 15, 16]
        );
        assert!(grid.finish_marquee());
        assert!(grid.marquee_bounds(25.0).is_none());
    }

    #[test]
    fn marquee_at_the_content_edge_requests_autoscroll_only_while_active() {
        let mut grid = GridInteraction::default();
        let status_height = 25.0;
        let start = Point::new(SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0, content_top() + 2.0);
        assert!(grid.start_marquee(start, 100, status_height, true));

        grid.move_cursor(
            Point::new(400.0, grid.window_size.height - status_height - 1.0),
            100,
        );
        assert!(grid.marquee_autoscroll(status_height) > 0.0);

        grid.move_cursor(
            Point::new(400.0, TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + 1.0),
            100,
        );
        assert!(grid.marquee_autoscroll(status_height) < 0.0);

        grid.move_cursor(Point::new(30.0, grid.window_size.height - 1.0), 100);
        assert_eq!(grid.marquee_autoscroll(status_height), 0.0);

        assert!(grid.finish_marquee());
        grid.move_cursor(
            Point::new(400.0, grid.window_size.height - status_height - 1.0),
            100,
        );
        assert_eq!(grid.marquee_autoscroll(status_height), 0.0);
    }

    #[test]
    fn stationary_empty_edge_clicks_do_not_autoscroll_before_drag_threshold() {
        let status_height = 25.0;
        let x = SIDEBAR_WIDTH + 2.0;
        let window_height = GridInteraction::default().window_size.height;

        for (y, direction) in [
            (content_top() + 2.0, -1.0),
            (window_height - status_height - 2.0, 1.0),
        ] {
            let mut grid = GridInteraction::default();
            let start = Point::new(x, y);
            grid.move_cursor(start, 100);
            assert!(grid.start_marquee(start, 100, status_height, true));

            assert_eq!(grid.marquee_autoscroll(status_height), 0.0);
            grid.move_cursor(Point::new(x + 5.0, y), 100);
            assert_eq!(grid.marquee_autoscroll(status_height), 0.0);
            grid.move_cursor(Point::new(x + 6.0, y), 100);
            assert_eq!(grid.marquee_autoscroll(status_height).signum(), direction);
        }
    }

    #[test]
    fn entry_scroll_recomputes_an_active_marquee_selection() {
        let mut grid = GridInteraction::default();
        let status_height = 25.0;
        let start = Point::new(SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0, content_top() + 2.0);
        assert!(grid.start_marquee(start, 30, status_height, true));
        grid.move_cursor(Point::new(400.0, 600.0), 30);
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [0, 1, 5, 6, 10, 11, 15, 16, 20, 21]
        );

        grid.observe_scroll(
            ScrollTarget::Entries,
            TILE_ROW_HEIGHT,
            10_000.0,
            Instant::now(),
            30,
        );

        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [0, 1, 5, 6, 10, 11, 15, 16, 20, 21, 25, 26]
        );
        assert_eq!(
            grid.marquee_bounds(status_height).unwrap().y,
            LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT
        );
    }

    #[test]
    fn scrolled_marquee_stays_below_the_sort_header() {
        let mut grid = GridInteraction::default();
        let status_height = 25.0;
        let start = Point::new(SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0, content_top() + 2.0);
        assert!(grid.start_marquee(start, 30, status_height, true));
        grid.move_cursor(Point::new(400.0, 600.0), 30);
        assert!(!grid.marquee_top_clipped());

        grid.observe_scroll(
            ScrollTarget::Entries,
            TILE_ROW_HEIGHT,
            10_000.0,
            Instant::now(),
            30,
        );

        assert_eq!(
            grid.marquee_bounds(status_height).unwrap().y,
            LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT
        );
        assert!(grid.marquee_top_clipped());
    }

    #[test]
    fn upward_scrolled_marquee_has_an_open_bottom_edge() {
        let mut grid = GridInteraction::default();
        let status_height = 25.0;
        grid.set_scroll(TILE_ROW_HEIGHT);
        let start = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0,
            grid.window_size.height - status_height - 2.0,
        );
        assert!(grid.start_marquee(start, 30, status_height, true));
        grid.move_cursor(Point::new(400.0, content_top() + 2.0), 30);
        assert!(!grid.marquee_bottom_clipped(status_height));

        grid.observe_scroll(
            ScrollTarget::Entries,
            -TILE_ROW_HEIGHT,
            10_000.0,
            Instant::now(),
            30,
        );

        assert!(grid.marquee_bottom_clipped(status_height));
    }

    #[test]
    fn marquee_maps_a_rectangular_grid_range_through_the_public_interaction() {
        let mut grid = grid();
        let content_top = content_top();
        let start = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + grid.column_width() * 1.5,
            content_top + TILE_HEIGHT + 2.0,
        );
        let end = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + grid.column_width() * 2.5,
            content_top + 2.0 * TILE_ROW_HEIGHT + 54.0,
        );

        assert!(grid.start_marquee(start, 8, 25.0, true));
        grid.move_cursor(end, 8);

        assert_eq!(grid.selected_entry(), Some(7));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [4, 5, 7]
        );
    }

    #[test]
    fn marquee_in_list_mode_uses_list_rows_and_starts_only_on_empty_space() {
        let mut grid = grid();
        grid.set_list_mode(true);
        let list_top =
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT;
        let entry_count = 5;
        let header_point = Point::new(SIDEBAR_WIDTH + CONTENT_GUTTER + 40.0, list_top - 2.0);
        let row_point = Point::new(
            grid.window_size.width - CONTENT_GUTTER - 2.0,
            list_top + 2.0 * LIST_ROW_HEIGHT + 2.0,
        );

        assert!(!grid.start_marquee(header_point, entry_count, 25.0, true));
        assert!(!grid.start_marquee(row_point, entry_count, 25.0, true));

        let start = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0,
            list_top + entry_count as f32 * LIST_ROW_HEIGHT + 8.0,
        );
        let end = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + 40.0,
            list_top + LIST_ROW_HEIGHT + 2.0,
        );
        assert!(grid.start_marquee(start, entry_count, 25.0, true));
        grid.move_cursor(end, entry_count);

        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn marquee_can_start_at_either_horizontal_edge_of_the_browser() {
        let mut grid = grid();
        grid.set_list_mode(true);
        let row_y = grid.entries_top() + LIST_ROW_HEIGHT / 2.0;
        let right_edge = grid.window_size.width - 1.0;
        let starts = [SIDEBAR_WIDTH + 1.0, right_edge].map(|x| {
            let started = grid.start_marquee(Point::new(x, row_y), 5, 25.0, true);
            grid.finish_marquee();
            started
        });

        assert_eq!(starts, [true, true]);
        assert!(!grid.start_marquee(
            Point::new(
                grid.window_size.width - CONTENT_GUTTER - SCROLLBAR_TRACK_WIDTH / 2.0,
                row_y,
            ),
            5,
            25.0,
            true,
        ));
    }

    #[test]
    fn marquee_can_start_next_to_the_status_bar_without_a_bottom_gutter() {
        let mut grid = GridInteraction::default();
        let status_height = 25.0;
        let point = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0,
            grid.window_size.height - status_height - 1.0,
        );

        assert!(grid.start_marquee(point, 0, status_height, true));
    }

    #[test]
    fn grid_sort_controls_do_not_start_a_marquee() {
        let mut grid = GridInteraction::default();
        let header = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + 8.0,
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + LIST_VIEW_TOP_INSET + 4.0,
        );

        assert!(!grid.start_marquee(header, 10, 25.0, true));
    }

    #[test]
    fn visible_range_accounts_for_scroll_and_short_content() {
        let mut grid = GridInteraction::default();
        grid.set_scroll(3.0 * TILE_ROW_HEIGHT);
        let visible = grid.visible_range(100, 25.0);

        assert_eq!(visible.first_index, 10);
        assert!(visible.last_index < 100);
        assert_eq!(visible.top_space, 2.0 * TILE_ROW_HEIGHT);
        assert!(visible.bottom_space > 0.0);
    }

    #[test]
    fn visible_ranges_clamp_stale_scroll_after_content_shrinks() {
        let mut grid = GridInteraction::default();
        grid.set_scroll(28.0 * TILE_ROW_HEIGHT);

        let tiles = grid.visible_range(19, 25.0);
        assert!(tiles.first_index <= tiles.last_index);
        assert!(tiles.last_index <= 19);

        let rows = grid.list_visible_range(19, 25.0);
        assert!(rows.start <= rows.end);
        assert!(rows.end <= 19);
    }

    #[test]
    fn drop_zones_distinguish_sidebar_tiles_empty_grid_and_chrome() {
        let grid = GridInteraction::default();
        assert_eq!(
            grid.drop_zone(Point::new(20.0, 50.0), 2, 3, 25.0, true),
            Some(DropZone::Sidebar(0))
        );
        assert_eq!(
            grid.drop_zone(Point::new(290.0, content_top() + 3.0), 2, 3, 25.0, true,),
            Some(DropZone::Entry(0))
        );
        assert_eq!(
            grid.drop_zone(Point::new(790.0, 500.0), 2, 3, 25.0, true),
            Some(DropZone::Current)
        );
        assert_eq!(
            grid.drop_zone(Point::new(300.0, 20.0), 2, 3, 25.0, true),
            None
        );
    }

    #[test]
    fn external_drag_bounds_include_every_window_edge() {
        let mut grid = GridInteraction::default();
        for point in [
            Point::new(-0.1, 20.0),
            Point::new(20.0, -0.1),
            Point::new(820.0, 20.0),
            Point::new(20.0, 560.0),
        ] {
            grid.move_cursor(point, 0);
            assert!(grid.cursor_outside_window(), "{point:?}");
        }

        grid.move_cursor(Point::new(819.9, 559.9), 0);
        assert!(!grid.cursor_outside_window());
    }

    #[test]
    fn drag_edges_scroll_only_the_surface_under_the_pointer() {
        let mut grid = GridInteraction::default();
        grid.move_cursor(Point::new(400.0, 535.0), 0);
        let (content, sidebar) = grid.drag_autoscroll(25.0);
        assert!(content > 0.0);
        assert_eq!(sidebar, 0.0);

        grid.move_cursor(Point::new(30.0, 31.0), 0);
        let (content, sidebar) = grid.drag_autoscroll(25.0);
        assert_eq!(content, 0.0);
        assert!(sidebar < 0.0);

        grid.move_cursor(Point::new(400.0, 250.0), 0);
        assert_eq!(grid.drag_autoscroll(25.0), (0.0, 0.0));
    }

    #[test]
    fn context_menu_owns_target_selection_and_keyboard_cursor() {
        let mut grid = GridInteraction::default();
        grid.move_cursor(Point::new(790.0, 500.0), 2);

        assert!(grid.open_background_context(2, 25.0));
        assert_eq!(grid.selected_entry(), None);
        assert_eq!(
            grid.context_menu().map(|menu| menu.target),
            Some(ContextTarget::Background)
        );
        assert_eq!(
            grid.navigate_context(ContextNavigation::Previous { wrap: true }, 3),
            ContextOutcome::None
        );
        assert_eq!(grid.context_menu().unwrap().focused, 2);
        assert_eq!(
            grid.navigate_context(ContextNavigation::Activate, 3),
            ContextOutcome::Activate(2)
        );
        assert_eq!(
            grid.navigate_context(ContextNavigation::Close, 3),
            ContextOutcome::Closed
        );
        assert!(grid.context_menu().is_none());
    }

    #[test]
    fn context_menu_pointer_focus_replaces_the_keyboard_focus() {
        let mut grid = GridInteraction::default();
        grid.move_cursor(Point::new(790.0, 500.0), 2);
        assert!(grid.open_background_context(2, 25.0));

        grid.navigate_context(ContextNavigation::Next { wrap: false }, 3);
        assert_eq!(grid.context_menu().unwrap().focused, 1);

        grid.focus_context(2, 3);
        assert_eq!(grid.context_menu().unwrap().focused, 2);

        grid.focus_context(usize::MAX, 3);
        assert_eq!(grid.context_menu().unwrap().focused, 2);
    }

    #[test]
    fn entry_context_rejects_missing_entries_and_selects_valid_target() {
        let mut grid = GridInteraction::default();

        assert!(!grid.open_entry_context(2, 2));
        assert!(grid.open_entry_context(1, 2));
        assert_eq!(grid.selected_entry(), Some(1));
        assert_eq!(grid.take_context_entry(), Some(1));
        assert!(grid.context_menu().is_none());
    }

    #[test]
    fn entry_context_preserves_a_selection_that_contains_its_target() {
        let mut grid = GridInteraction::default();
        grid.select_click(0, false, false, 3);
        grid.select_click(1, true, false, 3);
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [0, 1]
        );

        assert!(grid.open_entry_context(0, 3));

        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(grid.selected_entry(), Some(0));
        assert_eq!(
            grid.context_menu().map(|menu| menu.target),
            Some(ContextTarget::Entry(0))
        );

        grid.close_context();
        assert!(grid.open_entry_context(2, 3));
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [2]
        );
    }
}
