use super::*;
use iced::widget::{column, row};

impl App {
    pub(super) fn status_bar(&self) -> Element<'_, Message> {
        let height = self.status_height();
        let status_model = self.browser_status_model();
        let content: Element<'_, Message> = if status_model.presentation
            == BrowserStatusPresentation::Conflict
        {
            compact_status_line(
                text(status_model.text)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .color(self.accent_color())
                    .width(Fill),
            )
        } else if let Some(output) = self.command.output() {
            let header = row![
                text(&output.summary)
                    .font(MONO_FONT_SEMIBOLD)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .width(Fill),
                compact_text_button("Copy", Message::CopyCommandReport),
                text("Esc close")
                    .font(MONO_FONT)
                    .size(11)
                    .color(self.secondary_text_color()),
            ]
            .height(29)
            .align_y(Alignment::Center);
            let output = column![
                header,
                scrollable(
                    text(&output.detail)
                        .font(MONO_FONT)
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .color(with_alpha(self.iced_theme().palette().text, 0.84))
                        .width(Fill)
                        .wrapping(iced::advanced::text::Wrapping::WordOrGlyph),
                )
                .width(Fill)
                .height(Fill),
            ]
            .spacing(1)
            .width(Fill)
            .height(Fill);
            container(output)
                .width(Fill)
                .height(Fill)
                .padding(Padding {
                    top: 1.0,
                    right: CONTENT_GUTTER,
                    bottom: 9.0,
                    left: CONTENT_GUTTER,
                })
                .into()
        } else if self.file_operations.prompt_active() {
            self.prompt_bar()
        } else if self.transfers.expanded() && self.browser_input.mode() == InputMode::Browser {
            self.transfer_history_bar()
        } else {
            let status: Element<'_, Message> = match self.browser_input.mode() {
                InputMode::Search => {
                    let prefix = if self.search.is_recursive() {
                        "//"
                    } else {
                        "/"
                    };
                    row![
                        text(prefix)
                            .size(12)
                            .line_height(iced::Pixels(15.0))
                            .color(self.accent_color()),
                        text_input("", self.search.query())
                            .id(Id::new(SEARCH_ID))
                            .on_input(Message::SearchChanged)
                            .on_submit(Message::SearchSubmitted)
                            .font(MONO_FONT)
                            .size(12)
                            .line_height(iced::Pixels(15.0))
                            .padding(0)
                            .style(status_input_style)
                            .width(Fill),
                        self.search_count_view(),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into()
                }
                InputMode::Command => row![
                    text(self.command.prefix().unwrap_or(':').to_string())
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .color(self.accent_color()),
                    text_input("", self.command.text())
                        .id(Id::new(COMMAND_ID))
                        .on_input(Message::CommandChanged)
                        .on_submit(Message::CommandSubmitted)
                        .font(MONO_FONT)
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .padding(0)
                        .style(status_input_style)
                        .width(Fill),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into(),
                InputMode::Rename => {
                    if let FileOperationView::Rename { value, error } = self.file_operations.view()
                    {
                        let feedback: Element<'_, Message> = if self.busy {
                            self.spinner(13.0).into()
                        } else if error.is_empty() {
                            text("Enter save  ·  Esc cancel")
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(self.secondary_text_color())
                                .into()
                        } else {
                            text(error)
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(self.iced_theme().palette().danger)
                                .into()
                        };
                        row![
                            text("rename")
                                .font(MONO_FONT)
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(self.accent_color()),
                            text_input("", value)
                                .id(Id::new(RENAME_ID))
                                .on_input_maybe((!self.busy).then_some(Message::RenameChanged))
                                .on_submit_maybe((!self.busy).then_some(Message::RenameSubmitted))
                                .font(MONO_FONT)
                                .size(12)
                                .line_height(iced::Pixels(15.0))
                                .padding(0)
                                .style(status_input_style)
                                .width(Fill),
                            feedback,
                        ]
                        .spacing(7)
                        .align_y(Alignment::Center)
                        .into()
                    } else {
                        Space::new().into()
                    }
                }
                _ => {
                    if status_model.presentation == BrowserStatusPresentation::Transfer {
                        return container(compact_status_line(self.transfer_status_line()))
                            .width(Fill)
                            .height(Length::Fixed(height))
                            .clip(true)
                            .style(move |theme| {
                                status_background_style(
                                    theme,
                                    self.browser_focus == BrowserFocus::BottomBar,
                                )
                            })
                            .into();
                    }
                    let indicator: Element<'_, Message> = if self.busy || self.navigation.loading()
                    {
                        self.spinner(13.0).into()
                    } else {
                        Space::new().width(0).into()
                    };
                    let mut line = Row::new()
                        .push(indicator)
                        .push(
                            text(status_model.text)
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(if self.status_notice.is_some() {
                                    self.iced_theme().palette().danger
                                } else {
                                    self.secondary_text_color()
                                })
                                .width(Fill),
                        )
                        .spacing(if self.busy || self.navigation.loading() {
                            7
                        } else {
                            0
                        })
                        .align_y(Alignment::Center);
                    if status_model.retry {
                        line = line.push(compact_text_button("Retry", Message::RetryTransfer));
                    }
                    if status_model.history {
                        line = line.push(compact_text_button(
                            "History",
                            Message::ToggleTransferHistory,
                        ));
                    }
                    line.into()
                }
            };
            compact_status_line(status)
        };
        container(content)
            .width(Fill)
            .height(Length::Fixed(height))
            .clip(true)
            .style(move |theme| {
                status_background_style(theme, self.browser_focus == BrowserFocus::BottomBar)
            })
            .into()
    }

    pub(super) fn transfer_status_line(&self) -> Element<'_, Message> {
        let mut line = Row::new().spacing(8).align_y(Alignment::Center);
        if let Some(snapshot) = self.transfers.snapshot() {
            line = line
                .push(self.spinner(13.0))
                .push(
                    text(format_transfer_snapshot(
                        self.transfers.active_action().unwrap_or("Transfer"),
                        &snapshot,
                    ))
                    .font(MONO_FONT)
                    .size(11)
                    .width(Fill),
                )
                .push(compact_text_button("Cancel", Message::CancelTransfer));
        } else {
            line = line.push(
                text("Transfer finished with retained entries")
                    .font(MONO_FONT)
                    .size(11)
                    .width(Fill),
            );
        }
        if self.transfers.has_retry() {
            line = line.push(compact_text_button("Retry", Message::RetryTransfer));
        }
        line.push(compact_text_button(
            "History",
            Message::ToggleTransferHistory,
        ))
        .into()
    }

    pub(super) fn transfer_history_bar(&self) -> Element<'_, Message> {
        let mut header = Row::new()
            .push(
                text("transfers")
                    .font(MONO_FONT_SEMIBOLD)
                    .size(11)
                    .width(Fill),
            )
            .spacing(10)
            .height(25)
            .align_y(Alignment::Center);
        if self.transfers.active() {
            header = header.push(compact_text_button("Cancel", Message::CancelTransfer));
        }
        if self.transfers.has_retry() {
            header = header.push(compact_text_button("Retry", Message::RetryTransfer));
        }
        header = header
            .push(compact_text_button(
                "Copy report",
                Message::CopyTransferReport,
            ))
            .push(compact_text_button("Close", Message::ToggleTransferHistory));
        let active = self
            .transfers
            .snapshot()
            .map(|snapshot| {
                format_transfer_snapshot(
                    self.transfers.active_action().unwrap_or("Transfer"),
                    &snapshot,
                )
            })
            .into_iter();
        let history = self
            .transfers
            .history()
            .iter()
            .rev()
            .map(|entry| entry.summary().to_owned());
        let detail = active.chain(history).collect::<Vec<_>>().join("\n");
        let detail = if detail.is_empty() {
            "No transfer history".to_owned()
        } else {
            detail
        };
        container(
            column![
                header,
                scrollable(
                    text(detail)
                        .font(MONO_FONT)
                        .size(11)
                        .line_height(iced::Pixels(14.0))
                        .width(Fill),
                )
                .width(Fill)
                .height(Fill),
            ]
            .spacing(2),
        )
        .width(Fill)
        .height(Fill)
        .padding(Padding {
            top: 1.0,
            right: CONTENT_GUTTER,
            bottom: 7.0,
            left: CONTENT_GUTTER,
        })
        .into()
    }

    pub(super) fn prompt_bar(&self) -> Element<'_, Message> {
        match self.file_operations.view() {
            FileOperationView::NewFolder { value, error } => {
                self.name_prompt_bar("new folder", value, error)
            }
            FileOperationView::NewFile { value, error } => {
                self.name_prompt_bar("new file", value, error)
            }
            FileOperationView::Trash { message } => compact_status_line(
                row![
                    text("trash")
                        .font(MONO_FONT)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                    text(message)
                        .size(11)
                        .line_height(iced::Pixels(13.0))
                        .width(Fill),
                    text("Y/n")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ),
            FileOperationView::PermanentDelete { message, detail } => {
                let header = row![
                    text("delete permanently")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                    text(message).size(11).width(Fill),
                    text("Y/n")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                ]
                .spacing(8)
                .height(25)
                .align_y(Alignment::Center);
                container(
                    column![
                        header,
                        scrollable(
                            text(detail)
                                .font(MONO_FONT)
                                .size(11)
                                .line_height(iced::Pixels(14.0))
                                .color(self.secondary_text_color())
                                .width(Fill),
                        )
                        .width(Fill)
                        .height(Fill),
                    ]
                    .spacing(2),
                )
                .width(Fill)
                .height(Fill)
                .padding(Padding {
                    top: 1.0,
                    right: CONTENT_GUTTER,
                    bottom: 7.0,
                    left: CONTENT_GUTTER,
                })
                .into()
            }
            FileOperationView::Error { message } => {
                let header = row![
                    text("error")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                    Space::new().width(Fill),
                    text("Esc close")
                        .font(MONO_FONT)
                        .size(11)
                        .color(self.secondary_text_color()),
                ]
                .height(25)
                .align_y(Alignment::Center);
                container(
                    column![
                        header,
                        scrollable(
                            text(message)
                                .font(MONO_FONT)
                                .size(11)
                                .line_height(iced::Pixels(14.0))
                                .color(self.secondary_text_color())
                                .width(Fill),
                        )
                        .width(Fill)
                        .height(Fill),
                    ]
                    .spacing(2),
                )
                .width(Fill)
                .height(Fill)
                .padding(Padding {
                    top: 1.0,
                    right: CONTENT_GUTTER,
                    bottom: 7.0,
                    left: CONTENT_GUTTER,
                })
                .into()
            }
            FileOperationView::Idle | FileOperationView::Rename { .. } => Space::new().into(),
        }
    }

    pub(super) fn name_prompt_bar<'a>(
        &'a self,
        label: &'a str,
        value: &'a str,
        error: &'a str,
    ) -> Element<'a, Message> {
        let feedback: Element<'_, Message> = if self.busy {
            self.spinner(13.0).into()
        } else if error.is_empty() {
            text("Enter create  ·  Esc cancel")
                .font(MONO_FONT)
                .size(11)
                .color(self.secondary_text_color())
                .into()
        } else {
            text(error)
                .size(11)
                .line_height(iced::Pixels(13.0))
                .color(self.iced_theme().palette().danger)
                .into()
        };
        compact_status_line(
            row![
                text(label)
                    .font(MONO_FONT)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .color(self.accent_color()),
                text_input("", value)
                    .id(Id::new(NEW_FOLDER_ID))
                    .on_input_maybe((!self.busy).then_some(Message::PromptInputChanged))
                    .on_submit_maybe((!self.busy).then_some(Message::PromptSubmit))
                    .font(MONO_FONT)
                    .size(12)
                    .line_height(iced::Pixels(15.0))
                    .padding(0)
                    .style(status_input_style)
                    .width(Fill),
                feedback,
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
    }

    pub(super) fn search_count_view(&self) -> Element<'_, Message> {
        if !self.search.is_recursive() || self.search.query().is_empty() {
            return Space::new().into();
        }
        if self.search.is_loading() {
            return row![Space::new().width(Fill), self.spinner(13.0)]
                .width(108)
                .align_y(Alignment::Center)
                .into();
        }
        let label = if self.search.is_truncated() {
            "1000+ matches".to_owned()
        } else {
            format!("{} matches", self.navigation.entries().len())
        };
        text(label)
            .size(11)
            .line_height(iced::Pixels(13.0))
            .color(self.secondary_text_color())
            .into()
    }

    pub(super) fn context_menu_view(&self, point: Point) -> Element<'_, Message> {
        let mut actions = Column::new();
        for (index, (label, message)) in self.context_actions().into_iter().enumerate() {
            let focused = index == self.context_menu_cursor;
            actions = actions.push(
                button(text(label).size(13))
                    .on_press(message)
                    .style(move |theme, status| context_button_style(theme, status, focused))
                    .width(Fill),
            );
        }
        let menu = container(scrollable(actions).height(Length::Shrink))
            .width(220)
            .max_height(420)
            .padding(5)
            .style(menu_style);
        let overlay =
            mouse_area(container("").width(Fill).height(Fill)).on_press(Message::CloseContext);
        stack![overlay, pin(menu).x(point.x).y(point.y)].into()
    }

    pub(super) fn accent_color(&self) -> Color {
        if self.high_contrast() {
            return self.iced_theme().palette().primary;
        }
        self.accent
            .as_ref()
            .map_or(Color::from_rgb8(0, 120, 212), |colors| colors.accent)
    }

    pub(super) fn secondary_text_color(&self) -> Color {
        let mut color = self.iced_theme().palette().text;
        color.a = if self.high_contrast() || self.reduced_transparency() {
            1.0
        } else {
            0.62
        };
        color
    }

    pub(super) fn selection_text_color(&self) -> Color {
        if self.high_contrast() {
            return Color::BLACK;
        }
        self.accent
            .as_ref()
            .and_then(|colors| colors.selection_foreground)
            .unwrap_or(self.iced_theme().palette().text)
    }

    pub(super) fn entry_icon_color(&self, kind: EntryIconKind) -> Color {
        let palette = self.iced_theme().palette();
        match kind {
            EntryIconKind::Folder | EntryIconKind::Code => palette.primary,
            EntryIconKind::Image | EntryIconKind::Spreadsheet => palette.success,
            EntryIconKind::Pdf => palette.danger,
            EntryIconKind::Archive | EntryIconKind::Presentation => palette.warning,
            EntryIconKind::Audio | EntryIconKind::Video => Color::from_rgb8(164, 112, 218),
            EntryIconKind::Document | EntryIconKind::Generic => self.secondary_text_color(),
        }
    }
}
