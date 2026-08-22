use std::collections::BTreeSet;

use iced::{Point, Rectangle, Size};

pub(super) const SIDEBAR_WIDTH: f32 = 220.0;
pub(super) const TOOLBAR_HEIGHT: f32 = 46.0;
pub(super) const TOOLBAR_DIVIDER_HEIGHT: f32 = 1.0;
pub(super) const TILE_WIDTH: f32 = 104.0;
pub(super) const TILE_ROW_HEIGHT: f32 = 116.0;
pub(super) const CONTENT_GUTTER: f32 = 14.0;
pub(super) const LIST_VIEW_TOP_INSET: f32 = 6.0;

const TILE_PITCH: f32 = 112.0;
const TILE_HEIGHT: f32 = 108.0;
const TREE_TOP: f32 = 44.0;
const TREE_ROW_HEIGHT: f32 = 32.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DropZone {
    Sidebar(usize),
    Entry(usize),
    Current,
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
    current: Point,
}

#[derive(Clone, Debug)]
pub(super) struct GridInteraction {
    window_size: Size,
    scroll_y: f32,
    cursor: Point,
    marquee: Option<Marquee>,
    hovered: Option<usize>,
    selected: Option<usize>,
    selection: BTreeSet<usize>,
    visual_anchor: Option<usize>,
    details: Option<String>,
}

impl Default for GridInteraction {
    fn default() -> Self {
        Self::new(Size::new(820.0, 560.0))
    }
}

impl GridInteraction {
    pub(super) fn new(window_size: Size) -> Self {
        Self {
            window_size,
            scroll_y: 0.0,
            cursor: Point::ORIGIN,
            marquee: None,
            hovered: None,
            selected: None,
            selection: BTreeSet::new(),
            visual_anchor: None,
            details: None,
        }
    }

    pub(super) fn resize(&mut self, size: Size) {
        self.window_size = size;
    }

    pub(super) fn cursor(&self) -> Point {
        self.cursor
    }

    pub(super) fn move_cursor(&mut self, position: Point, entry_count: usize) -> bool {
        self.cursor = position;
        if let Some(marquee) = &mut self.marquee {
            marquee.current = position;
            self.update_marquee_selection(entry_count);
            true
        } else {
            false
        }
    }

    pub(super) fn move_pointer_in_grid(&mut self, point: Point, entry_count: usize) -> bool {
        let Some(marquee) = &mut self.marquee else {
            return false;
        };
        marquee.current = Point::new(
            point.x + SIDEBAR_WIDTH,
            point.y + TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT,
        );
        self.update_marquee_selection(entry_count);
        true
    }

    pub(super) fn set_scroll(&mut self, y: f32) {
        self.scroll_y = y;
    }

    pub(super) fn reset_scroll(&mut self) {
        self.scroll_y = 0.0;
    }

    #[cfg(test)]
    pub(super) fn scroll_offset(&self) -> f32 {
        self.scroll_y
    }

    pub(super) fn enter(&mut self, index: usize) {
        self.hovered = Some(index);
    }

    pub(super) fn leave(&mut self, index: usize) {
        if self.hovered == Some(index) {
            self.hovered = None;
        }
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
    }

    pub(super) fn select_indices(&mut self, indices: &[usize], entry_count: usize) {
        self.selection = indices
            .iter()
            .copied()
            .filter(|index| *index < entry_count)
            .collect();
        self.selected = self.selection.first().copied();
        self.visual_anchor = None;
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
            current: point,
        });
        self.update_marquee_selection(entry_count);
        true
    }

    pub(super) fn finish_marquee(&mut self) -> bool {
        self.marquee.take().is_some()
    }

    pub(super) fn marquee_bounds(&self, status_height: f32) -> Option<Rectangle> {
        let marquee = self.marquee.as_ref()?;
        let origin = Point::new(SIDEBAR_WIDTH, TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT);
        let size = Size::new(
            (self.window_size.width - origin.x).max(0.0),
            (self.window_size.height - origin.y - status_height).max(0.0),
        );
        let left = (marquee.start.x.min(marquee.current.x) - origin.x).clamp(0.0, size.width);
        let right = (marquee.start.x.max(marquee.current.x) - origin.x).clamp(0.0, size.width);
        let top = (marquee.start.y.min(marquee.current.y) - origin.y).clamp(0.0, size.height);
        let bottom = (marquee.start.y.max(marquee.current.y) - origin.y).clamp(0.0, size.height);
        Some(Rectangle::new(
            Point::new(left, top),
            Size::new(right - left, bottom - top),
        ))
    }

    pub(super) fn columns(&self) -> usize {
        let width = (self.window_size.width - SIDEBAR_WIDTH - 2.0 * CONTENT_GUTTER).max(1.0);
        (width / TILE_PITCH).floor().max(1.0) as usize
    }

    pub(super) fn column_width(&self) -> f32 {
        let width = (self.window_size.width - SIDEBAR_WIDTH - 2.0 * CONTENT_GUTTER).max(1.0);
        width / self.columns() as f32
    }

    pub(super) fn scroll_target(&self, index: usize) -> f32 {
        (index / self.columns()) as f32 * TILE_ROW_HEIGHT
    }

    pub(super) fn visible_range(&self, entry_count: usize, status_height: f32) -> VisibleRange {
        let columns = self.columns();
        let total_rows = entry_count.div_ceil(columns);
        let viewport_height = (self.window_size.height
            - TOOLBAR_HEIGHT
            - TOOLBAR_DIVIDER_HEIGHT
            - status_height
            - 2.0 * CONTENT_GUTTER
            - LIST_VIEW_TOP_INSET)
            .max(TILE_ROW_HEIGHT);
        let first_row = ((self.scroll_y / TILE_ROW_HEIGHT).floor() as usize).saturating_sub(1);
        let visible_rows = (viewport_height / TILE_ROW_HEIGHT).ceil() as usize + 2;
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
        if point.x < SIDEBAR_WIDTH {
            let index = ((point.y - TREE_TOP) / TREE_ROW_HEIGHT).floor() as isize;
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
            && self.point_over_tile(point, index)
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
        let content_top = content_top();
        if point.x < SIDEBAR_WIDTH + CONTENT_GUTTER
            || point.x >= self.window_size.width - CONTENT_GUTTER
            || point.y < content_top
            || point.y >= self.window_size.height - status_height - CONTENT_GUTTER
        {
            return false;
        }
        let Some(index) = self.index_at(point, entry_count) else {
            return true;
        };
        !self.point_over_tile(point, index)
    }

    fn cell_at(&self, point: Point) -> Option<(i32, i32)> {
        if point.x < SIDEBAR_WIDTH + CONTENT_GUTTER || point.y < content_top() {
            return None;
        }
        let column =
            ((point.x - SIDEBAR_WIDTH - CONTENT_GUTTER) / self.column_width()).floor() as i32;
        let row = ((point.y - content_top() + self.scroll_y) / TILE_ROW_HEIGHT).floor() as i32;
        (row >= 0 && column >= 0 && column < self.columns() as i32).then_some((row, column))
    }

    fn selection_cell_at(&self, point: Point) -> (i32, i32) {
        let column =
            ((point.x - SIDEBAR_WIDTH - CONTENT_GUTTER) / self.column_width()).floor() as i32;
        let row = ((point.y - content_top() + self.scroll_y) / TILE_ROW_HEIGHT).floor() as i32;
        (row, column)
    }

    fn index_at(&self, point: Point, entry_count: usize) -> Option<usize> {
        let (row, column) = self.cell_at(point)?;
        let index = row as usize * self.columns() + column as usize;
        (index < entry_count).then_some(index)
    }

    fn point_over_tile(&self, point: Point, index: usize) -> bool {
        let columns = self.columns();
        let row = index / columns;
        let column = index % columns;
        let column_left = SIDEBAR_WIDTH + CONTENT_GUTTER + column as f32 * self.column_width();
        let tile_left = column_left + (self.column_width() - TILE_WIDTH) / 2.0;
        let row_top = content_top() - self.scroll_y + row as f32 * TILE_ROW_HEIGHT;
        point.x >= tile_left
            && point.x < tile_left + TILE_WIDTH
            && point.y >= row_top
            && point.y < row_top + TILE_HEIGHT
    }

    fn update_marquee_selection(&mut self, entry_count: usize) {
        let Some(marquee) = &self.marquee else {
            return;
        };
        let (start_row, start_column) = self.selection_cell_at(marquee.start);
        let (end_row, end_column) = self.selection_cell_at(marquee.current);
        self.select_rectangle(start_row, start_column, end_row, end_column, entry_count);
    }

    fn select_rectangle(
        &mut self,
        start_row: i32,
        start_column: i32,
        end_row: i32,
        end_column: i32,
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
                if index < entry_count {
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

fn content_top() -> f32 {
    TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + CONTENT_GUTTER + LIST_VIEW_TOP_INSET
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> GridInteraction {
        GridInteraction::new(Size::new(584.0, 560.0))
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
        assert_eq!(bounds.height, 466.0);
        assert_eq!(
            grid.selected_indices().iter().copied().collect::<Vec<_>>(),
            [5, 6, 10, 11, 15, 16]
        );
        assert!(grid.finish_marquee());
        assert!(grid.marquee_bounds(25.0).is_none());
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
            [1, 2, 4, 5, 7]
        );
    }

    #[test]
    fn grid_local_pointer_coordinates_update_the_window_marquee() {
        let mut grid = GridInteraction::default();
        let start = Point::new(300.0, 177.0);
        assert!(grid.start_marquee(start, 10, 25.0, true));

        assert!(grid.move_pointer_in_grid(Point::new(203.0, 53.0), 10));
        let bounds = grid.marquee_bounds(25.0).unwrap();
        assert_eq!(bounds.width, 123.0);
        assert_eq!(bounds.height, 77.0);
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
    fn drop_zones_distinguish_sidebar_tiles_empty_grid_and_chrome() {
        let grid = GridInteraction::default();
        assert_eq!(
            grid.drop_zone(Point::new(20.0, 50.0), 2, 3, 25.0, true),
            Some(DropZone::Sidebar(0))
        );
        assert_eq!(
            grid.drop_zone(Point::new(290.0, 70.0), 2, 3, 25.0, true),
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
}
