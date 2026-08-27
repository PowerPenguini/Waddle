use super::*;
use iced::widget::{column, row};
use std::ops::Deref;

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

    fn view(self) -> Element<'a, Message> {
        let base = self.layout();
        let mut layers: Vec<Element<'_, Message>> = vec![base];
        if let Some(preview) = self.drag_preview_view() {
            layers.push(preview);
        }
        if let Some(menu) = self.grid.context_menu() {
            layers.push(self.context_menu_view(menu));
        }
        stack(layers).width(Fill).height(Fill).into()
    }

    fn drag_preview_view(self) -> Option<Element<'a, Message>> {
        let entries = self.transfers.overview().pointer_drag.entries();
        let preview = self.drag_preview(entries)?;
        let preview_bytes = native_dnd::preview_svg(preview).ok()?;
        let preview = svg(svg::Handle::from_memory(preview_bytes))
            .width(native_dnd::ICON_SIZE as f32)
            .height(native_dnd::ICON_SIZE as f32);

        let origin = self.grid.drag_preview_origin();
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
        let palette = self.iced_theme().palette();
        let background = palette.background;
        let icon_color = self.entry_icon_color(icon_kind);
        let accent = self.accent_color();
        let badge_text = self.selection_text_color();
        Some(TransferPreview {
            icon: entry_icon_asset(icon_kind),
            count: entries.len(),
            copy: self.modifiers.control(),
            background: rgba(background, 0.92),
            icon_color: rgba(icon_color, 1.0),
            accent: rgba(accent, 1.0),
            badge_text: rgba(badge_text, 1.0),
        })
    }

    fn layout(self) -> Element<'a, Message> {
        if self.view_preferences.tree_visible() {
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
        for tree_row in self.sidebar_tree.rows(self.navigation.current()) {
            rows = rows.push(self.tree_row(tree_row));
        }
        let scrollbar_opacity = self.grid.scrollbar_opacity(
            Scrollbar::Sidebar,
            self.presentation.now(),
            self.reduced_motion(),
        );
        let content = column![
            container(
                text("Locations")
                    .font(UI_FONT_SEMIBOLD)
                    .size(12)
                    .line_height(iced::Pixels(14.0))
                    .color(with_alpha(self.iced_theme().palette().text, 0.68)),
            )
            .height(30)
            .center_y(30),
            scrollable(container(rows).padding(Padding {
                top: LIST_VIEW_TOP_INSET,
                ..Padding::ZERO
            }))
            .id(Id::new(SIDEBAR_SCROLL_ID))
            .on_scroll(|viewport| Message::SidebarScrolled(viewport.absolute_offset().y))
            .direction(transient_vertical_scrollbar())
            .style(move |theme, status| {
                transient_scrollbar_style(theme, status, scrollbar_opacity)
            })
            .height(Fill),
        ];
        container(content)
            .width(SIDEBAR_WIDTH)
            .height(Fill)
            .padding(Padding::from([8, 12]))
            .style(move |theme| {
                sidebar_style(theme, self.high_contrast() || self.reduced_transparency())
            })
            .into()
    }

    fn tree_row(self, tree_row: TreeRow) -> Element<'a, Message> {
        let drop_target = self.drop_highlight_path().as_deref() == Some(tree_row.path.as_path());
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
        let icon = tree_icon_asset(tree_row.kind);
        let icon_color = if matches!(
            tree_row.kind,
            tree::NodeKind::Computer | tree::NodeKind::Drive
        ) {
            self.secondary_text_color()
        } else {
            self.accent_color()
        };
        let selected = tree_row.selected;
        let focused = self.presentation.focus_is(BrowserFocus::Sidebar) && tree_row.focused;
        let label_color = if selected || focused {
            self.selection_text_color()
        } else {
            self.iced_theme().palette().text
        };
        if tree_row.loading {
            line = line.push(self.spinner(17.0));
        } else {
            line = line.push(themed_svg(icon, 17.0, icon_color));
        }
        line = line.push(
            text(tree_row.label)
                .size(13)
                .line_height(iced::Pixels(16.0))
                .color(label_color)
                .width(Fill)
                .height(Fill)
                .align_y(Alignment::Center),
        );
        let content = column![line.height(30), self.drag_activation_bar(&tree_row.path)].spacing(0);
        let button = button(content)
            .on_press(Message::TreeRow(tree_row.id))
            .width(Fill)
            .height(32)
            .padding(0)
            .style(move |theme, status| {
                tree_button_style(theme, status, selected, focused, drop_target)
            });
        if let Some(index) = tree_row.favorite_index {
            mouse_area(button)
                .on_press(Message::FavoritePressed(index))
                .on_release(Message::FavoriteReleased(index))
                .into()
        } else {
            button.into()
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
            self.navigation.folder_displayed() && self.navigation.current().parent().is_some(),
            Message::Parent,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.presentation.focus_is(BrowserFocus::Toolbar)
                && self.presentation.toolbar_cursor() == 0,
        );
        let back = toolbar_button(
            include_bytes!("../ui/icons/back.svg"),
            "Back",
            self.navigation.can_go_back(),
            Message::Back,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.presentation.focus_is(BrowserFocus::Toolbar)
                && self.presentation.toolbar_cursor() == 1,
        );
        let forward = toolbar_button(
            include_bytes!("../ui/icons/forward.svg"),
            "Forward",
            self.navigation.can_go_forward(),
            Message::Forward,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.presentation.focus_is(BrowserFocus::Toolbar)
                && self.presentation.toolbar_cursor() == 2,
        );
        let refresh = toolbar_button(
            include_bytes!("../ui/icons/refresh.svg"),
            "Refresh",
            !self.navigation.loading(),
            Message::Refresh,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.presentation.focus_is(BrowserFocus::Toolbar)
                && self.presentation.toolbar_cursor() == 3,
        );
        let options = self
            .view_preferences
            .for_directory(self.navigation.current());
        let location: Element<'_, Message> = if !self.navigation.folder_displayed() {
            let label = self.navigation.location_label();
            container(text(label).font(UI_FONT_SEMIBOLD).size(13))
                .width(Fill)
                .height(34)
                .padding(Padding::from([0, 7]))
                .center_y(34)
                .into()
        } else {
            let input = text_input("Location", &self.location_input)
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
            if self.browser_input.mode() == InputMode::Location {
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
                focus_container_style(theme, self.presentation.focus_is(BrowserFocus::Location))
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
                    self.iced_theme().palette().text,
                    self.iced_theme().palette().background,
                    self.presentation.focus_is(BrowserFocus::Toolbar)
                        && self.presentation.toolbar_cursor() == 4,
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
            .view_preferences
            .for_directory(self.navigation.current())
            .view
            == fs::ViewMode::List
        {
            return self.list_body();
        }
        let visible = self
            .grid
            .visible_range(self.navigation.entries().len(), self.status_height());
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
        let scrollbar_opacity = self.grid.scrollbar_opacity(
            Scrollbar::Entries,
            self.presentation.now(),
            self.reduced_motion(),
        );
        let scroll = scrollable(content)
            .id(Id::new(GRID_SCROLL_ID))
            .on_scroll(|viewport| Message::GridScrolled(viewport.absolute_offset().y))
            .direction(transient_vertical_scrollbar())
            .style(move |theme, status| transient_scrollbar_style(theme, status, scrollbar_opacity))
            .width(Fill)
            .height(Fill);
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
                container(scroll)
                    .padding(Padding {
                        top: CONTENT_GUTTER,
                        ..Padding::ZERO
                    })
                    .width(Fill)
                    .height(Fill),
            ])
            .padding(Padding {
                top: LIST_VIEW_TOP_INSET,
                right: CONTENT_GUTTER,
                bottom: CONTENT_GUTTER,
                left: CONTENT_GUTTER,
            })
            .width(Fill)
            .height(Fill),
        )
        .on_move(Message::GridPointerMoved)
        .into();
        let content = self.with_marquee(area);
        let current_drop_target =
            self.drop_highlight_path().as_deref() == Some(self.navigation.current());
        container(content)
            .width(Fill)
            .height(Fill)
            .clip(true)
            .style(move |theme| {
                grid_background_style(
                    theme,
                    current_drop_target,
                    self.presentation.focus_is(BrowserFocus::Entries),
                )
            })
            .into()
    }

    fn list_body(self) -> Element<'a, Message> {
        let visible = self
            .grid
            .list_visible_range(self.navigation.entries().len(), self.status_height());
        let top = Space::new()
            .height(visible.start as f32 * LIST_ROW_HEIGHT)
            .width(Fill);
        let bottom = Space::new()
            .height(
                self.navigation.entries().len().saturating_sub(visible.end) as f32
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
        let scrollbar_opacity = self.grid.scrollbar_opacity(
            Scrollbar::Entries,
            self.presentation.now(),
            self.reduced_motion(),
        );
        let area: Element<'_, Message> = mouse_area(
            container(column![
                container(header)
                    .height(LIST_HEADER_HEIGHT)
                    .padding(Padding::from([0, LIST_HORIZONTAL_PADDING])),
                scrollable(rows)
                    .id(Id::new(GRID_SCROLL_ID))
                    .on_scroll(|viewport| Message::GridScrolled(viewport.absolute_offset().y))
                    .direction(transient_vertical_scrollbar())
                    .style(move |theme, status| {
                        transient_scrollbar_style(theme, status, scrollbar_opacity)
                    })
                    .width(Fill)
                    .height(Fill),
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
                    self.presentation.focus_is(BrowserFocus::Entries),
                )
            }),
        )
        .on_move(Message::GridPointerMoved)
        .into();
        self.with_marquee(area)
    }

    fn with_marquee(self, area: Element<'a, Message>) -> Element<'a, Message> {
        let Some(bounds) = self.grid.marquee_bounds(self.status_height()) else {
            return area;
        };
        let selection = container(Space::new())
            .width(bounds.width)
            .height(bounds.height)
            .style(move |_| marquee_style(self.accent_color()));
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
            .view_preferences
            .for_directory(self.navigation.current());
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
                            self.iced_theme().palette().text
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
        let width = self.grid.window_width() + (SIDEBAR_WIDTH - self.grid.sidebar_width());
        (width >= LIST_SHOW_SIZE_AT, width >= LIST_SHOW_MODIFIED_AT)
    }

    fn list_name_character_limit(self) -> usize {
        let (show_size, show_modified) = self.list_column_visibility();
        let fixed_width = self.grid.sidebar_width()
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
        let available = (self.grid.window_width() - fixed_width - spacing)
            .max(LIST_NAME_MIN_CHARACTERS as f32 * LIST_NAME_APPROX_CHARACTER_WIDTH);
        (available / LIST_NAME_APPROX_CHARACTER_WIDTH).floor() as usize
    }

    fn file_list_row(self, index: usize) -> Element<'a, Message> {
        let entry = &self.navigation.entries()[index];
        let selected = self.grid.is_selected(index);
        let selected_above = index
            .checked_sub(1)
            .is_some_and(|neighbor| self.grid.is_selected(neighbor));
        let selected_below = self.grid.is_selected(index.saturating_add(1));
        let focused = self.presentation.focus_is(BrowserFocus::Entries)
            && self.grid.selected_entry() == Some(index);
        let hovered = self.grid.hovered() == Some(index);
        let content_opacity = entry_content_opacity(
            entry.is_hidden(),
            selected || hovered || focused,
            self.reduced_transparency(),
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
            .push(
                entry_svg(
                    icon_kind,
                    LIST_ENTRY_ICON_WIDTH,
                    self.entry_icon_color(icon_kind),
                )
                .opacity(content_opacity),
            )
            .push(
                container(
                    text(name)
                        .size(13)
                        .color(apply_opacity(
                            self.iced_theme().palette().text,
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
        let entry = &self.navigation.entries()[index];
        let label = tile_label(&clip_file_name(
            &fs::display_name(&entry.name),
            GRID_NAME_MAX_CHARACTERS,
        ));
        let selected = self.grid.is_selected(index);
        let focused = self.presentation.focus_is(BrowserFocus::Entries)
            && self.grid.selected_entry() == Some(index);
        let hovered = self.grid.hovered() == Some(index);
        let drop_target = entry.is_directory()
            && self.drop_highlight_path().as_deref() == Some(entry.path.as_path());
        let content_opacity = entry_content_opacity(
            entry.is_hidden(),
            selected || hovered || focused || drop_target,
            self.reduced_transparency(),
        );
        let icon_kind = entry_icon_kind(entry);
        let icon: Element<'_, Message> = self.thumbnails.handle(&entry.path).map_or_else(
            || {
                entry_svg(icon_kind, 48.0, self.entry_icon_color(icon_kind))
                    .opacity(content_opacity)
                    .into()
            },
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
            self.iced_theme().palette().text
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
        let Some(progress) = self.grid.drag_hover_progress(path, Instant::now()) else {
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

impl Deref for View<'_> {
    type Target = App;

    fn deref(&self) -> &Self::Target {
        self.app
    }
}
