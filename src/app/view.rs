use std::path::Path;

use iced::{
    Alignment, Element, Fill, Length, Padding,
    time::Instant,
    widget::{
        self, Column, Grid, Id, Row, Space, button, column, container, mouse_area, pin,
        progress_bar, row, rule, scrollable, stack, svg, text, text_input,
    },
};

use crate::{fs, fs::FileEntry, transfer::Preview as TransferPreview};

use super::wheel_area::wheel_area;
use super::{
    App, BrowserFocus, CONTENT_GUTTER, EntryIconKind, GRID_NAME_MAX_CHARACTERS, GRID_SCROLL_ID,
    InputMode, LIST_COLUMN_SPACING, LIST_ENTRY_ICON_WIDTH, LIST_HEADER_HEIGHT,
    LIST_HEADER_HORIZONTAL_PADDING, LIST_HEADER_ICON_SLOT_WIDTH, LIST_HORIZONTAL_PADDING,
    LIST_MODIFIED_WIDTH, LIST_NAME_APPROX_CHARACTER_WIDTH, LIST_NAME_MIN_CHARACTERS,
    LIST_ROW_HEIGHT, LIST_SHOW_MODIFIED_AT, LIST_SHOW_SIZE_AT, LIST_SIZE_WIDTH, LIST_TYPE_WIDTH,
    LIST_VIEW_TOP_INSET, LOCATION_ID, MONO_FONT, Message, SIDEBAR_SCROLL_ID, SIDEBAR_WIDTH,
    ScrollTarget, TILE_HEIGHT, TILE_ROW_HEIGHT, TILE_WIDTH, TOOLBAR_HEIGHT, TreeRow, UI_FONT,
    UI_FONT_SEMIBOLD, apply_opacity, browser_background_style, clip_file_name,
    entry_content_opacity, entry_icon_asset, entry_icon_kind, entry_svg, flat_input_style,
    focus_container_style, format_storage_usage, grid_background_style, list_row_style,
    marquee_style, native_dnd, rgba, sidebar_style, solid_background_style, themed_svg, tile_label,
    tile_style, toolbar_button, toolbar_button_style, transient_scrollbar_style,
    transient_vertical_scrollbar, tree, tree_button_style, tree_icon_asset,
    tree_unmount_button_style, with_alpha,
};

const TREE_LABEL_ROOT_MAX_CHARACTERS: usize = 23;
const TREE_LABEL_DEPTH_CHARACTER_COST: usize = 2;
const TREE_LABEL_MIN_CHARACTERS: usize = 8;

fn clip_tree_label(label: &str, depth: usize) -> String {
    let max_characters = TREE_LABEL_ROOT_MAX_CHARACTERS
        .saturating_sub(depth.saturating_mul(TREE_LABEL_DEPTH_CHARACTER_COST))
        .max(TREE_LABEL_MIN_CHARACTERS);
    clip_file_name(label, max_characters)
}

fn scrolled(target: ScrollTarget, viewport: scrollable::Viewport) -> Message {
    Message::Scrolled {
        target,
        offset: viewport.absolute_offset().y,
        maximum: (viewport.content_bounds().height - viewport.bounds().height).max(0.0),
    }
}

pub(super) const GRID_SORT_CONTROLS: [(&str, fs::SortKey); 4] = [
    ("Name", fs::SortKey::Name),
    ("Type", fs::SortKey::Type),
    ("Size", fs::SortKey::Size),
    ("Modified", fs::SortKey::Modified),
];

#[derive(Clone, Copy)]
pub(super) struct View<'a> {
    app: &'a App,
}

impl<'a> View<'a> {
    pub(super) fn new(app: &'a App) -> Self {
        Self { app }
    }

    pub(super) fn app(self) -> &'a App {
        self.app
    }

    fn entry_icon(self, kind: EntryIconKind, size: f32, opacity: f32) -> Element<'static, Message> {
        self.system_icon(super::system_icons::Kind::Entry(kind), size, opacity)
            .unwrap_or_else(|| {
                entry_svg(kind, size, self.entry_icon_color(kind))
                    .opacity(opacity)
                    .into()
            })
    }

    fn tree_icon(
        self,
        kind: tree::NodeKind,
        size: f32,
        color: iced::Color,
    ) -> Element<'static, Message> {
        self.system_icon(super::system_icons::Kind::Tree(kind), size, 1.0)
            .unwrap_or_else(|| themed_svg(tree_icon_asset(kind), size, color).into())
    }

    fn system_icon(
        self,
        kind: super::system_icons::Kind,
        size: f32,
        opacity: f32,
    ) -> Option<Element<'static, Message>> {
        if !self.app.view_preferences.uses_system_icons() {
            return None;
        }
        match self.app.system_icons.resolve(kind, size.round() as u16)? {
            super::system_icons::Asset::Svg(handle) => {
                Some(svg(handle).width(size).height(size).opacity(opacity).into())
            }
            super::system_icons::Asset::Raster(handle) => Some(
                widget::image(handle)
                    .width(size)
                    .height(size)
                    .content_fit(iced::ContentFit::Contain)
                    .opacity(opacity)
                    .into(),
            ),
        }
    }

    pub(super) fn render(self) -> Element<'a, Message> {
        self.view()
    }

    pub(super) fn transfer_preview(app: &'a App, entries: &[FileEntry]) -> Option<TransferPreview> {
        Self::new(app).drag_preview(entries)
    }

    #[cfg(test)]
    pub(super) fn drag_preview_layer(app: &'a App) -> Option<Element<'a, Message>> {
        Self::new(app).drag_preview_view()
    }

    #[cfg(test)]
    pub(super) fn list_metrics(app: &'a App) -> ((bool, bool), usize) {
        let view = Self::new(app);
        (
            view.list_column_visibility(),
            view.list_name_character_limit(),
        )
    }

    #[cfg(test)]
    pub(super) fn empty_folder_state_visible(app: &'a App) -> bool {
        Self::new(app).shows_empty_folder_state()
    }

    fn view(self) -> Element<'a, Message> {
        let base = self.layout();
        let mut layers: Vec<Element<'_, Message>> = vec![base];
        if let Some(preview) = self.drag_preview_view() {
            layers.push(preview);
        }
        if let Some(menu) = self.app.grid.context_menu() {
            layers.push(self.context_menu_view(menu));
        }
        stack(layers).width(Fill).height(Fill).into()
    }

    fn drag_preview_view(self) -> Option<Element<'a, Message>> {
        let entries = self.app.transfers.overview().pointer_drag.entries();
        let preview = self.drag_preview(entries)?;
        let preview = svg(self.app.drag_preview.resolve(preview).ok()?)
            .width(native_dnd::ICON_SIZE as f32)
            .height(native_dnd::ICON_SIZE as f32);

        let origin = self.app.grid.drag_preview_origin();
        Some(
            pin(preview)
                .x(origin.x.max(0.0))
                .y(origin.y.max(0.0))
                .into(),
        )
    }

    fn drag_preview(self, entries: &[FileEntry]) -> Option<TransferPreview> {
        let first = entries.first()?;
        let icon_kind = entry_icon_kind(first);
        let palette = self.app.iced_theme().palette();
        let background = palette.background;
        let icon_color = self.entry_icon_color(icon_kind);
        let accent = self.accent_color();
        let badge_text = self.selection_text_color();
        Some(TransferPreview {
            icon: entry_icon_asset(icon_kind),
            count: entries.len(),
            copy: self.app.modifiers.control(),
            background: rgba(background, 0.92),
            icon_color: rgba(icon_color, 1.0),
            accent: rgba(accent, 1.0),
            badge_text: rgba(badge_text, 1.0),
        })
    }

    fn layout(self) -> Element<'a, Message> {
        if self.app.view_preferences.tree_visible() {
            row![self.sidebar(), self.browser()]
                .spacing(0)
                .width(Fill)
                .height(Fill)
                .into()
        } else {
            self.browser()
        }
    }

    fn sidebar(self) -> Element<'a, Message> {
        let mut rows = Column::new().spacing(0);
        let mut previous_section = None;
        for tree_row in self.app.sidebar_tree.rows(self.app.navigation.current()) {
            if tree_row.depth == 0 {
                let section = tree::sidebar_section(tree_row.kind);
                if previous_section.is_some_and(|previous| previous != section) {
                    rows = rows.push(self.sidebar_separator());
                }
                previous_section = Some(section);
            }
            rows = rows.push(self.tree_row(tree_row));
        }
        let scrollbar_opacity = self.app.grid.scrollbar_opacity(
            ScrollTarget::Sidebar,
            self.app.presentation.now(),
            self.app.reduced_motion(),
        );
        let scroll = scrollable(container(rows).padding(Padding {
            top: LIST_VIEW_TOP_INSET,
            ..Padding::ZERO
        }))
        .id(Id::new(SIDEBAR_SCROLL_ID))
        .on_scroll(|viewport| scrolled(ScrollTarget::Sidebar, viewport))
        .direction(transient_vertical_scrollbar())
        .style(move |theme, status| transient_scrollbar_style(theme, status, scrollbar_opacity))
        .height(Fill);
        let scroll = wheel_area(scroll, ScrollTarget::Sidebar);
        let content = column![
            container(
                text("Locations")
                    .font(UI_FONT_SEMIBOLD)
                    .size(12)
                    .line_height(iced::Pixels(14.0))
                    .color(with_alpha(self.app.iced_theme().palette().text, 0.68)),
            )
            .height(30)
            .center_y(30),
            scroll,
        ];
        container(content)
            .width(SIDEBAR_WIDTH)
            .height(Fill)
            .padding(Padding::from([8, 12]))
            .style(move |theme| {
                sidebar_style(
                    theme,
                    self.app.high_contrast() || self.app.reduced_transparency(),
                )
            })
            .into()
    }

    fn sidebar_separator(self) -> Element<'a, Message> {
        container(
            rule::horizontal(1).style(move |theme: &iced::Theme| rule::Style {
                color: with_alpha(theme.palette().text, 0.12),
                radius: Default::default(),
                fill_mode: rule::FillMode::Padded(5),
                snap: true,
            }),
        )
        .width(Fill)
        .height(13)
        .center_y(13)
        .into()
    }

    fn tree_row(self, tree_row: TreeRow) -> Element<'a, Message> {
        let row_height = tree_row.height();
        let shows_storage_usage = tree_row.shows_storage_usage();
        let unmount = tree_row
            .volume_id
            .clone()
            .filter(|_| tree_row.can_unmount && !tree_row.loading);
        let drop_target = tree_row
            .path
            .as_deref()
            .is_some_and(|path| self.app.drop_highlight_path().as_deref() == Some(path));
        let mut line = Row::new()
            .spacing(6)
            .height(Fill)
            .align_y(Alignment::Center)
            .padding(0);
        line = line.push(
            Space::new()
                .width((tree_row.depth as f32 * 16.0) + 5.0)
                .height(1),
        );
        let icon_color = if matches!(
            tree_row.kind,
            tree::NodeKind::Computer | tree::NodeKind::Drive
        ) {
            self.secondary_text_color()
        } else {
            self.accent_color()
        };
        let selected = tree_row.selected;
        let focused = self.app.presentation.focus_is(BrowserFocus::Sidebar) && tree_row.focused;
        let label_color = if selected || focused {
            self.selection_text_color()
        } else {
            self.app.iced_theme().palette().text
        };
        let label = clip_tree_label(&tree_row.label, tree_row.depth);
        if tree_row.loading {
            line = line.push(self.app.spinner(17.0));
        } else {
            line = line.push(self.tree_icon(tree_row.kind, 17.0, icon_color));
        }
        line = line.push(
            container(
                text(label)
                    .size(13)
                    .line_height(iced::Pixels(16.0))
                    .color(label_color)
                    .wrapping(iced::advanced::text::Wrapping::None),
            )
            .width(Fill)
            .height(Fill)
            .align_y(Alignment::Center)
            .clip(true),
        );
        if unmount.is_some() {
            line = line.push(Space::new().width(28).height(1));
        }
        let activation_bar = tree_row.path.as_deref().map_or_else(
            || Space::new().width(Fill).height(2).into(),
            |path| self.drag_activation_bar(path),
        );
        let mut content = Column::new()
            .spacing(0)
            .push(line.height(if shows_storage_usage { 27 } else { 30 }));
        if shows_storage_usage {
            let muted_text = with_alpha(label_color, 0.62);
            let track_color = with_alpha(label_color, 0.12);
            let bar_color = if selected || focused {
                with_alpha(label_color, 0.82)
            } else {
                self.accent_color()
            };
            let used_fraction = tree_row
                .storage_usage
                .map_or(0.0, fs::StorageUsage::used_fraction);
            let bar = progress_bar(0.0..=1.0, used_fraction)
                .girth(4)
                .style(move |_| widget::progress_bar::Style {
                    background: track_color.into(),
                    bar: bar_color.into(),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..iced::Border::default()
                    },
                });
            let details = tree_row
                .storage_usage
                .map_or_else(|| "Reading storage…".to_owned(), format_storage_usage);
            let metrics = column![
                container(
                    text(details)
                        .size(10)
                        .line_height(iced::Pixels(12.0))
                        .color(muted_text)
                        .wrapping(iced::advanced::text::Wrapping::None),
                )
                .width(Fill)
                .height(12)
                .padding(Padding {
                    top: 0.0,
                    right: 5.0,
                    bottom: 0.0,
                    left: (tree_row.depth as f32 * 16.0) + 12.0,
                })
                .clip(true),
                container(bar).width(Fill).height(4).padding(Padding {
                    top: 0.0,
                    right: 5.0,
                    bottom: 0.0,
                    left: (tree_row.depth as f32 * 16.0) + 12.0,
                }),
            ]
            .spacing(4);
            content = content.push(
                container(metrics)
                    .width(Fill)
                    .height(27)
                    .padding(Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 4.0,
                        left: 0.0,
                    })
                    .clip(true),
            );
        }
        let content = content.push(activation_bar);
        let row_button = button(content)
            .on_press(Message::TreeRow(tree_row.id))
            .width(Fill)
            .height(row_height)
            .padding(0)
            .style(move |theme, status| {
                tree_button_style(theme, status, selected, focused, drop_target)
            });
        let row: Element<'a, Message> = if let Some(volume_id) = unmount {
            let unmount_button = button(themed_svg(
                include_bytes!("../ui/icons/eject.svg"),
                15.0,
                label_color,
            ))
            .on_press(Message::TreeVolumeUnmount(volume_id))
            .width(28)
            .height(28)
            .padding(6)
            .style(tree_unmount_button_style);
            let unmount_button = widget::tooltip(
                unmount_button,
                container(text(format!("Unmount {}", tree_row.label)).size(12))
                    .padding([4, 7])
                    .style(container::rounded_box),
                widget::tooltip::Position::Bottom,
            );
            stack![
                row_button,
                container(unmount_button)
                    .width(Fill)
                    .height(row_height)
                    .padding(2)
                    .align_x(Alignment::End)
                    .align_y(Alignment::Start),
            ]
            .into()
        } else {
            row_button.into()
        };
        if let Some(index) = tree_row.favorite_index {
            mouse_area(row)
                .on_press(Message::FavoritePressed(index))
                .on_release(Message::FavoriteReleased(index))
                .into()
        } else {
            row
        }
    }

    fn browser(self) -> Element<'a, Message> {
        column![
            self.toolbar(),
            rule::horizontal(1),
            self.grid_body(),
            self.status_bar(),
        ]
        .spacing(0)
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn toolbar(self) -> Element<'a, Message> {
        let parent = toolbar_button(
            include_bytes!("../ui/icons/up.svg"),
            "Parent folder",
            self.app.navigation.folder_displayed()
                && self.app.navigation.current().parent().is_some(),
            Message::Parent,
            self.app.iced_theme().palette().text,
            self.app.iced_theme().palette().background,
            self.app.presentation.focus_is(BrowserFocus::Toolbar)
                && self.app.presentation.toolbar_cursor() == 0,
        );
        let back = toolbar_button(
            include_bytes!("../ui/icons/back.svg"),
            "Back",
            self.app.navigation.loading() || self.app.navigation.can_go_back(),
            Message::Back,
            self.app.iced_theme().palette().text,
            self.app.iced_theme().palette().background,
            self.app.presentation.focus_is(BrowserFocus::Toolbar)
                && self.app.presentation.toolbar_cursor() == 1,
        );
        let forward = toolbar_button(
            include_bytes!("../ui/icons/forward.svg"),
            "Forward",
            self.app.navigation.can_go_forward(),
            Message::Forward,
            self.app.iced_theme().palette().text,
            self.app.iced_theme().palette().background,
            self.app.presentation.focus_is(BrowserFocus::Toolbar)
                && self.app.presentation.toolbar_cursor() == 2,
        );
        let refresh = toolbar_button(
            include_bytes!("../ui/icons/refresh.svg"),
            "Refresh",
            !self.app.navigation.loading(),
            Message::Refresh,
            self.app.iced_theme().palette().text,
            self.app.iced_theme().palette().background,
            self.app.presentation.focus_is(BrowserFocus::Toolbar)
                && self.app.presentation.toolbar_cursor() == 3,
        );
        let options = self
            .app
            .view_preferences
            .for_directory(self.app.navigation.current());
        let location: Element<'_, Message> = if !self.app.navigation.folder_displayed() {
            let label = self.app.navigation.location_label();
            container(text(label).font(UI_FONT_SEMIBOLD).size(13))
                .width(Fill)
                .height(34)
                .padding(Padding::from([0, 7]))
                .center_y(34)
                .into()
        } else {
            let input = text_input("Location", &self.app.location_input)
                .id(Id::new(LOCATION_ID))
                .on_input(Message::LocationChanged)
                .on_submit(Message::LocationSubmitted)
                .on_paste(Message::LocationChanged)
                .font(UI_FONT)
                .padding(Padding::from([0, 10]))
                .size(14)
                .line_height(iced::Pixels(17.0))
                .style(flat_input_style)
                .width(Fill);
            if self.app.browser_input.mode() == InputMode::Location {
                let accent = self.accent_color();
                column![
                    container(input).width(Fill).height(33).center_y(33),
                    container(Space::new().width(Fill).height(1))
                        .width(Fill)
                        .height(1)
                        .style(move |_| solid_background_style(accent)),
                ]
                .spacing(0)
                .width(Fill)
                .height(34)
                .into()
            } else {
                container(input).width(Fill).height(34).center_y(34).into()
            }
        };
        let location = container(location)
            .width(Fill)
            .height(34)
            .style(move |theme| {
                focus_container_style(
                    theme,
                    self.app.presentation.focus_is(BrowserFocus::Location),
                )
            });
        container(
            row![
                parent,
                back,
                forward,
                refresh,
                location,
                toolbar_button(
                    match options.view {
                        fs::ViewMode::Grid => include_bytes!("../ui/icons/view-grid.svg"),
                        fs::ViewMode::List => include_bytes!("../ui/icons/view-list.svg"),
                    },
                    "Toggle view",
                    true,
                    Message::ToggleView,
                    self.app.iced_theme().palette().text,
                    self.app.iced_theme().palette().background,
                    self.app.presentation.focus_is(BrowserFocus::Toolbar)
                        && self.app.presentation.toolbar_cursor() == 4,
                ),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .height(TOOLBAR_HEIGHT)
        .padding(Padding::from([6, CONTENT_GUTTER as u16]))
        .style(browser_background_style)
        .into()
    }

    fn grid_body(self) -> Element<'a, Message> {
        if self
            .app
            .view_preferences
            .for_directory(self.app.navigation.current())
            .view
            == fs::ViewMode::List
        {
            return self.list_body();
        }
        let visible = self.app.grid.visible_range(
            self.app.navigation.entries().len(),
            self.app.status_height(),
        );
        let mut grid = Grid::with_capacity(visible.last_index.saturating_sub(visible.first_index))
            .columns(visible.columns)
            .height(widget::grid::aspect_ratio(
                visible.column_width,
                TILE_ROW_HEIGHT,
            ))
            .spacing(0);
        for index in visible.first_index..visible.last_index {
            grid = grid.push(self.file_tile(index));
        }
        let top = Space::new().width(Fill).height(visible.top_space);
        let bottom = Space::new().width(Fill).height(visible.bottom_space);
        let content = column![top, grid, bottom];
        let scrollbar_opacity = self.app.grid.scrollbar_opacity(
            ScrollTarget::Entries,
            self.app.presentation.now(),
            self.app.reduced_motion(),
        );
        let scroll = scrollable(content)
            .id(Id::new(GRID_SCROLL_ID))
            .on_scroll(|viewport| scrolled(ScrollTarget::Entries, viewport))
            .direction(transient_vertical_scrollbar())
            .style(move |theme, status| transient_scrollbar_style(theme, status, scrollbar_opacity))
            .width(Fill)
            .height(Fill);
        let entries: Element<'_, Message> = if self.shows_empty_folder_state() {
            self.empty_folder_state()
        } else {
            wheel_area(
                container(scroll).width(Fill).height(Fill),
                ScrollTarget::Entries,
            )
        };
        let sort_controls = GRID_SORT_CONTROLS.into_iter().fold(
            Row::new()
                .spacing(4)
                .width(Fill)
                .height(LIST_HEADER_HEIGHT)
                .align_y(Alignment::Center),
            |controls, (label, sort)| {
                controls.push(self.sort_header(label, sort, Length::FillPortion(1)))
            },
        );
        let area: Element<'_, Message> = mouse_area(
            container(column![
                container(sort_controls)
                    .height(LIST_HEADER_HEIGHT)
                    .padding(Padding::from([0, LIST_HORIZONTAL_PADDING])),
                entries,
            ])
            .padding(Padding {
                top: LIST_VIEW_TOP_INSET,
                right: CONTENT_GUTTER,
                bottom: 0.0,
                left: CONTENT_GUTTER,
            })
            .width(Fill)
            .height(Fill),
        )
        .into();
        let content = self.with_marquee(area);
        let current_drop_target =
            self.app.drop_highlight_path().as_deref() == Some(self.app.navigation.current());
        container(content)
            .width(Fill)
            .height(Fill)
            .clip(true)
            .style(move |theme| {
                grid_background_style(
                    theme,
                    current_drop_target,
                    self.app.presentation.focus_is(BrowserFocus::Entries),
                )
            })
            .into()
    }

    fn list_body(self) -> Element<'a, Message> {
        let visible = self.app.grid.list_visible_range(
            self.app.navigation.entries().len(),
            self.app.status_height(),
        );
        let top = Space::new()
            .height(visible.start as f32 * LIST_ROW_HEIGHT)
            .width(Fill);
        let bottom = Space::new()
            .height(
                self.app
                    .navigation
                    .entries()
                    .len()
                    .saturating_sub(visible.end) as f32
                    * LIST_ROW_HEIGHT,
            )
            .width(Fill);
        let mut rows = Column::new().spacing(0).push(top);
        for index in visible {
            rows = rows.push(self.file_list_row(index));
        }
        rows = rows.push(bottom);
        let (show_size, show_modified) = self.list_column_visibility();
        let mut header = Row::new()
            .push(Space::new().width(LIST_HEADER_ICON_SLOT_WIDTH))
            .push(self.sort_header("Name", fs::SortKey::Name, Fill))
            .push(self.sort_header("Type", fs::SortKey::Type, LIST_TYPE_WIDTH))
            .width(Fill)
            .spacing(LIST_COLUMN_SPACING)
            .align_y(Alignment::Center);
        if show_size {
            header = header.push(self.sort_header("Size", fs::SortKey::Size, LIST_SIZE_WIDTH));
        }
        if show_modified {
            header = header.push(self.sort_header(
                "Modified",
                fs::SortKey::Modified,
                LIST_MODIFIED_WIDTH,
            ));
        }
        let scrollbar_opacity = self.app.grid.scrollbar_opacity(
            ScrollTarget::Entries,
            self.app.presentation.now(),
            self.app.reduced_motion(),
        );
        let entries: Element<'_, Message> = if self.shows_empty_folder_state() {
            self.empty_folder_state()
        } else {
            let scroll = scrollable(rows)
                .id(Id::new(GRID_SCROLL_ID))
                .on_scroll(|viewport| scrolled(ScrollTarget::Entries, viewport))
                .direction(transient_vertical_scrollbar())
                .style(move |theme, status| {
                    transient_scrollbar_style(theme, status, scrollbar_opacity)
                })
                .width(Fill)
                .height(Fill);
            wheel_area(scroll, ScrollTarget::Entries)
        };
        let area: Element<'_, Message> = mouse_area(
            container(column![
                container(header)
                    .height(LIST_HEADER_HEIGHT)
                    .padding(Padding::from([0, LIST_HORIZONTAL_PADDING])),
                entries,
            ])
            .padding(Padding::from([
                LIST_VIEW_TOP_INSET as u16,
                CONTENT_GUTTER as u16,
            ]))
            .width(Fill)
            .height(Fill)
            .style(move |theme| {
                grid_background_style(
                    theme,
                    false,
                    self.app.presentation.focus_is(BrowserFocus::Entries),
                )
            }),
        )
        .into();
        self.with_marquee(area)
    }

    fn shows_empty_folder_state(self) -> bool {
        self.app.navigation.folder_displayed()
            && self.app.navigation.entries().is_empty()
            && !self.app.navigation.loading()
            && !self.app.search.is_recursive()
    }

    fn empty_folder_state(self) -> Element<'a, Message> {
        let icon = self.entry_icon(EntryIconKind::Folder, 44.0, 0.38);
        let label = text("This folder is empty")
            .font(UI_FONT)
            .size(13)
            .color(apply_opacity(self.secondary_text_color(), 0.82));
        container(column![icon, label].spacing(12).align_x(Alignment::Center))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    }

    fn with_marquee(self, area: Element<'a, Message>) -> Element<'a, Message> {
        let Some(bounds) = self.app.grid.marquee_bounds(self.app.status_height()) else {
            return stack![area].into();
        };
        let accent = self.accent_color();
        let fill: Element<'_, Message> = container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(move |_| marquee_style(accent))
            .into();
        let left: Element<'_, Message> = container(Space::new())
            .width(1)
            .height(Fill)
            .style(move |_| solid_background_style(accent))
            .into();
        let right: Element<'_, Message> = container(
            container(Space::new())
                .width(1)
                .height(Fill)
                .style(move |_| solid_background_style(accent)),
        )
        .align_right(Fill)
        .height(Fill)
        .into();
        let mut selection_layers = vec![fill, left, right];
        if !self
            .app
            .grid
            .marquee_bottom_clipped(self.app.status_height())
        {
            selection_layers.push(
                container(
                    container(Space::new())
                        .width(Fill)
                        .height(1)
                        .style(move |_| solid_background_style(accent)),
                )
                .width(Fill)
                .align_bottom(Fill)
                .into(),
            );
        }
        if !self.app.grid.marquee_top_clipped() {
            selection_layers.push(
                container(Space::new())
                    .width(Fill)
                    .height(1)
                    .style(move |_| solid_background_style(accent))
                    .into(),
            );
        }
        let selection = stack(selection_layers)
            .width(bounds.width)
            .height(bounds.height);
        let overlay = column![
            Space::new().height(bounds.y),
            row![
                Space::new().width(bounds.x),
                selection,
                Space::new().width(Fill)
            ]
            .height(bounds.height),
            Space::new().height(Fill),
        ]
        .width(Fill)
        .height(Fill);
        stack![area, overlay].into()
    }

    fn sort_header<'b>(
        self,
        label: &'b str,
        sort: fs::SortKey,
        width: impl Into<Length>,
    ) -> Element<'b, Message> {
        let options = self
            .app
            .view_preferences
            .for_directory(self.app.navigation.current());
        let active = options.sort == sort;
        let direction = if active {
            if options.descending { " ↓" } else { " ↑" }
        } else {
            ""
        };
        container(
            button(
                container(
                    text(format!("{label}{direction}"))
                        .font(MONO_FONT)
                        .size(10)
                        .wrapping(iced::advanced::text::Wrapping::None)
                        .color(if active {
                            self.app.iced_theme().palette().text
                        } else {
                            self.secondary_text_color()
                        }),
                )
                .width(Fill)
                .height(Fill)
                .align_y(Alignment::Center),
            )
            .on_press(Message::SortBy(sort))
            .padding(Padding::from([0, LIST_HEADER_HORIZONTAL_PADDING]))
            .height(26)
            .width(Fill)
            .style(toolbar_button_style),
        )
        .width(width)
        .height(26)
        .clip(true)
        .into()
    }

    fn list_column_visibility(self) -> (bool, bool) {
        let width = self.app.grid.window_width() + (SIDEBAR_WIDTH - self.app.grid.sidebar_width());
        (width >= LIST_SHOW_SIZE_AT, width >= LIST_SHOW_MODIFIED_AT)
    }

    fn list_name_character_limit(self) -> usize {
        let (show_size, show_modified) = self.list_column_visibility();
        let fixed_width = self.app.grid.sidebar_width()
            + CONTENT_GUTTER * 2.0
            + f32::from(LIST_HORIZONTAL_PADDING) * 2.0
            + LIST_ENTRY_ICON_WIDTH
            + LIST_TYPE_WIDTH
            + if show_size { LIST_SIZE_WIDTH } else { 0.0 }
            + if show_modified {
                LIST_MODIFIED_WIDTH
            } else {
                0.0
            };
        let column_count = 3 + usize::from(show_size) + usize::from(show_modified);
        let spacing = (column_count.saturating_sub(1)) as f32 * LIST_COLUMN_SPACING;
        let available = (self.app.grid.window_width() - fixed_width - spacing)
            .max(LIST_NAME_MIN_CHARACTERS as f32 * LIST_NAME_APPROX_CHARACTER_WIDTH);
        (available / LIST_NAME_APPROX_CHARACTER_WIDTH).floor() as usize
    }

    fn file_list_row(self, index: usize) -> Element<'a, Message> {
        let entry = &self.app.navigation.entries()[index];
        let selected = self.app.grid.is_selected(index);
        let selected_above = index
            .checked_sub(1)
            .is_some_and(|neighbor| self.app.grid.is_selected(neighbor));
        let selected_below = self.app.grid.is_selected(index.saturating_add(1));
        let focused = self.app.presentation.focus_is(BrowserFocus::Entries)
            && self.app.grid.selected_entry() == Some(index);
        let hovered = self.app.grid.hovered() == Some(index);
        let content_opacity = entry_content_opacity(
            entry.is_hidden(),
            selected || hovered || focused,
            self.app.reduced_transparency(),
        );
        let icon_kind = entry_icon_kind(entry);
        let kind = entry.type_label();
        let size = if entry.is_directory() {
            "—".to_owned()
        } else {
            entry
                .metadata
                .size
                .map(fs::format_size)
                .unwrap_or_else(|| "—".to_owned())
        };
        let modified = entry.metadata.modified.map_or_else(
            || "—".to_owned(),
            |seconds| {
                gio::glib::DateTime::from_unix_local(seconds)
                    .and_then(|date| date.format("%Y-%m-%d %H:%M"))
                    .map_or_else(|_| "—".to_owned(), |date| date.to_string())
            },
        );
        let (show_size, show_modified) = self.list_column_visibility();
        let name = clip_file_name(
            &fs::display_name(&entry.name),
            self.list_name_character_limit(),
        );
        let mut content = Row::new()
            .push(self.entry_icon(icon_kind, LIST_ENTRY_ICON_WIDTH, content_opacity))
            .push(
                container(
                    text(name)
                        .size(13)
                        .color(apply_opacity(
                            self.app.iced_theme().palette().text,
                            content_opacity,
                        ))
                        .wrapping(iced::advanced::text::Wrapping::None),
                )
                .width(Fill)
                .clip(true),
            )
            .push(
                text(kind)
                    .font(MONO_FONT)
                    .size(11)
                    .color(apply_opacity(self.secondary_text_color(), content_opacity))
                    .width(LIST_TYPE_WIDTH),
            )
            .width(Fill)
            .spacing(LIST_COLUMN_SPACING)
            .align_y(Alignment::Center);
        if show_size {
            content = content.push(
                text(size)
                    .font(MONO_FONT)
                    .size(11)
                    .color(apply_opacity(self.secondary_text_color(), content_opacity))
                    .width(LIST_SIZE_WIDTH),
            );
        }
        if show_modified {
            content = content.push(
                text(modified)
                    .font(MONO_FONT)
                    .size(11)
                    .color(apply_opacity(self.secondary_text_color(), content_opacity))
                    .width(LIST_MODIFIED_WIDTH),
            );
        }
        let content = column![
            content.height(LIST_ROW_HEIGHT - 2.0),
            self.drag_activation_bar(&entry.path)
        ]
        .spacing(0);
        let row = container(content)
            .height(LIST_ROW_HEIGHT)
            .padding(Padding::from([0, LIST_HORIZONTAL_PADDING]))
            .style(move |theme| {
                list_row_style(
                    theme,
                    selected,
                    hovered,
                    focused,
                    selected_above,
                    selected_below,
                )
            });
        mouse_area(row)
            .on_press(Message::EntryPressed(index))
            .on_release(Message::EntryReleased(index))
            .on_double_click(Message::EntryDoubleClicked(index))
            .on_right_press(Message::EntryContext(index))
            .into()
    }

    fn file_tile(self, index: usize) -> Element<'a, Message> {
        let entry = &self.app.navigation.entries()[index];
        let label = tile_label(&clip_file_name(
            &fs::display_name(&entry.name),
            GRID_NAME_MAX_CHARACTERS,
        ));
        let selected = self.app.grid.is_selected(index);
        let focused = self.app.presentation.focus_is(BrowserFocus::Entries)
            && self.app.grid.selected_entry() == Some(index);
        let hovered = self.app.grid.hovered() == Some(index);
        let drop_target = entry.is_directory()
            && self.app.drop_highlight_path().as_deref() == Some(entry.path.as_path());
        let content_opacity = entry_content_opacity(
            entry.is_hidden(),
            selected || hovered || focused || drop_target,
            self.app.reduced_transparency(),
        );
        let icon_kind = entry_icon_kind(entry);
        let icon: Element<'_, Message> = self.app.thumbnails.handle(&entry.path).map_or_else(
            || self.entry_icon(icon_kind, 48.0, content_opacity),
            |handle| {
                widget::image(handle.clone())
                    .width(48)
                    .height(48)
                    .content_fit(iced::ContentFit::Cover)
                    .border_radius(5)
                    .opacity(content_opacity)
                    .into()
            },
        );
        let label_color = if selected {
            self.selection_text_color()
        } else {
            self.app.iced_theme().palette().text
        };
        let content = column![
            container(icon)
                .width(Fill)
                .height(48)
                .center_x(Fill)
                .center_y(48),
            container(
                text(label)
                    .font(UI_FONT)
                    .size(13)
                    .line_height(iced::Pixels(16.0))
                    .color(apply_opacity(label_color, content_opacity))
                    .width(Fill)
                    .height(34)
                    .wrapping(iced::advanced::text::Wrapping::WordOrGlyph)
                    .align_x(Alignment::Center),
            )
            .width(Fill)
            .height(34)
            .clip(true),
            self.drag_activation_bar(&entry.path),
        ]
        .spacing(4)
        .align_x(Alignment::Center);
        let tile = container(content)
            .width(TILE_WIDTH)
            .height(TILE_HEIGHT)
            .padding(Padding {
                top: 10.0,
                right: 7.0,
                bottom: 6.0,
                left: 7.0,
            })
            .clip(true)
            .style(move |theme| tile_style(theme, selected, hovered, focused, drop_target));
        let tile = mouse_area(tile)
            .on_press(Message::EntryPressed(index))
            .on_release(Message::EntryReleased(index))
            .on_double_click(Message::EntryDoubleClicked(index))
            .on_right_press(Message::EntryContext(index));
        container(tile).width(Fill).center_x(Fill).into()
    }

    fn drag_activation_bar(self, path: &Path) -> Element<'a, Message> {
        let Some(progress) = self.app.grid.drag_hover_progress(path, Instant::now()) else {
            return Space::new().width(Fill).height(2).into();
        };
        let filled = ((progress * 100.0).round() as u16).clamp(1, 99);
        row![
            container(Space::new())
                .width(Length::FillPortion(filled))
                .height(2)
                .style(move |_| solid_background_style(self.accent_color())),
            Space::new()
                .width(Length::FillPortion(100 - filled))
                .height(2),
        ]
        .spacing(0)
        .width(Fill)
        .height(2)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use iced::Point;
    use iced::advanced::widget::Tree;

    use super::*;
    use crate::app::grid::TOOLBAR_DIVIDER_HEIGHT;

    #[test]
    fn tree_labels_end_with_an_ellipsis_before_the_sidebar_edge() {
        assert_eq!(clip_tree_label("Pictures", 0), "Pictures");
        assert_eq!(
            clip_tree_label("Waddle-LinkedIn-2026-08-28", 1),
            "Waddle-LinkedIn-2026…"
        );
    }

    #[test]
    fn sidebar_sections_separate_computer_places_utilities_and_devices() {
        assert_eq!(
            [
                tree::NodeKind::Computer,
                tree::NodeKind::Home,
                tree::NodeKind::Favorite,
                tree::NodeKind::Recent,
                tree::NodeKind::Trash,
                tree::NodeKind::Drive,
            ]
            .map(tree::sidebar_section),
            [
                tree::SidebarSection::Computer,
                tree::SidebarSection::Places,
                tree::SidebarSection::Places,
                tree::SidebarSection::Utilities,
                tree::SidebarSection::Utilities,
                tree::SidebarSection::Devices,
            ]
        );
    }

    #[test]
    fn marquee_overlay_keeps_the_scrollable_widget_root_stable() {
        let (mut app, _) = App::new();
        let area =
            || -> Element<'_, Message> { scrollable(Space::new()).width(Fill).height(Fill).into() };
        let before = View::new(&app).with_marquee(area());
        let root_tag = Tree::new(before.as_widget()).tag;
        drop(before);

        let point = Point::new(
            SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0,
            TOOLBAR_HEIGHT
                + TOOLBAR_DIVIDER_HEIGHT
                + LIST_VIEW_TOP_INSET
                + LIST_HEADER_HEIGHT
                + 2.0,
        );
        assert!(app.grid.start_marquee(point, 0, app.status_height(), true));
        let active = View::new(&app).with_marquee(area());

        assert_eq!(Tree::new(active.as_widget()).tag, root_tag);
        drop(active);

        assert!(app.grid.finish_marquee());
        let finished = View::new(&app).with_marquee(area());
        assert_eq!(Tree::new(finished.as_widget()).tag, root_tag);
    }
}
