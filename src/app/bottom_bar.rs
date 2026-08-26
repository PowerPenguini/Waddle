use super::view::View;
use super::*;
use iced::widget::{column, row};

pub(super) fn command_output_action_spacing() -> f32 {
    12.0
}

impl<'a> View<'a> {
    pub(super) fn status_bar(self) -> Element<'a, Message> {
        let height = self.status_height();
        let status_model = self.app().browser_status_model();
        let transient = self.app().transient_presentation().kind();
        let content: Element<'_, Message> = if transient == TransientPresentationKind::Conflict {
            compact_status_line(
                text(status_model.text)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .color(self.accent_color())
                    .width(Fill),
            )
        } else if transient == TransientPresentationKind::OpenWith {
            self.open_with_bar()
        } else if transient == TransientPresentationKind::CommandOutput {
            let output = self
                .app()
                .command
                .output()
                .expect("command output transient must have output");
            let header = row![
                text(&output.summary)
                    .font(MONO_FONT_SEMIBOLD)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .width(Fill),
                text("y copy")
                    .font(MONO_FONT)
                    .size(11)
                    .color(self.secondary_text_color()),
                text("Esc close")
                    .font(MONO_FONT)
                    .size(11)
                    .color(self.secondary_text_color()),
            ]
            .spacing(command_output_action_spacing())
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
        } else if transient == TransientPresentationKind::FileOperation {
            self.prompt_bar()
        } else if transient == TransientPresentationKind::TransferHistory {
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
                    if let FileOperationView::Rename { value, error } =
                        self.app().file_operations.view()
                    {
                        let feedback: Element<'_, Message> = if self.foreground_operation_active() {
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
                                .on_input_maybe(
                                    (!self.foreground_operation_active())
                                        .then_some(Message::RenameChanged),
                                )
                                .on_submit_maybe(
                                    (!self.foreground_operation_active())
                                        .then_some(Message::RenameSubmitted),
                                )
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
                                    self.presentation.focus_is(BrowserFocus::BottomBar),
                                    self.presentation
                                        .copy_feedback_intensity(self.reduced_motion()),
                                )
                            })
                            .into();
                    }
                    let indicator: Element<'_, Message> =
                        if self.foreground_operation_active() || self.navigation.loading() {
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
                                .color(if self.presentation.notice().is_some() {
                                    self.iced_theme().palette().danger
                                } else {
                                    self.secondary_text_color()
                                })
                                .width(Fill),
                        )
                        .spacing(
                            if self.foreground_operation_active() || self.navigation.loading() {
                                7
                            } else {
                                0
                            },
                        )
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
                status_background_style(
                    theme,
                    self.presentation.focus_is(BrowserFocus::BottomBar),
                    self.presentation
                        .copy_feedback_intensity(self.reduced_motion()),
                )
            })
            .into()
    }

    fn transfer_status_line(self) -> Element<'a, Message> {
        let transfers = self.transfers.overview();
        let mut line = Row::new().spacing(8).align_y(Alignment::Center);
        if let Some(snapshot) = transfers.snapshot {
            line = line
                .push(self.spinner(13.0))
                .push(
                    text(format_transfer_snapshot(
                        transfers.active_action.unwrap_or("Transfer"),
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
        if transfers.retry {
            line = line.push(compact_text_button("Retry", Message::RetryTransfer));
        }
        line.push(compact_text_button(
            "History",
            Message::ToggleTransferHistory,
        ))
        .into()
    }

    fn transfer_history_bar(self) -> Element<'a, Message> {
        let transfers = self.transfers.overview();
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
        if transfers.active {
            header = header.push(compact_text_button("Cancel", Message::CancelTransfer));
        }
        if transfers.retry {
            header = header.push(compact_text_button("Retry", Message::RetryTransfer));
        }
        header = header
            .push(compact_text_button(
                "Copy report",
                Message::CopyTransferReport,
            ))
            .push(compact_text_button("Close", Message::ToggleTransferHistory));
        let active = transfers
            .snapshot
            .map(|snapshot| {
                format_transfer_snapshot(transfers.active_action.unwrap_or("Transfer"), &snapshot)
            })
            .into_iter();
        let history = transfers
            .history
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

    fn prompt_bar(self) -> Element<'a, Message> {
        match self.app().file_operations.view() {
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

    fn open_with_bar(self) -> Element<'a, Message> {
        let open_with::View::Open {
            target_name,
            applications,
            custom,
            error,
        } = self.app().open_with.view()
        else {
            return Space::new().into();
        };

        let header = row![
            text("open with")
                .font(MONO_FONT_SEMIBOLD)
                .size(11)
                .color(self.accent_color()),
            text(target_name)
                .size(11)
                .line_height(iced::Pixels(13.0))
                .width(Fill),
            text("Esc cancel")
                .font(MONO_FONT)
                .size(11)
                .color(self.secondary_text_color()),
        ]
        .spacing(8)
        .height(25)
        .align_y(Alignment::Center);

        let options: Element<'_, Message> = if applications.is_empty() {
            container(
                text("No compatible applications found — enter one below")
                    .size(11)
                    .color(self.secondary_text_color()),
            )
            .height(Fill)
            .align_y(Alignment::Center)
            .into()
        } else {
            let mut rows = Column::new().spacing(1);
            for application in applications {
                let default: Element<'_, Message> = if application.default {
                    text("default")
                        .font(MONO_FONT)
                        .size(10)
                        .color(self.accent_color())
                        .into()
                } else {
                    Space::new().width(0).into()
                };
                let content = row![
                    text(&application.name)
                        .size(12)
                        .line_height(iced::Pixels(14.0))
                        .width(Length::FillPortion(2)),
                    text(&application.id)
                        .font(MONO_FONT)
                        .size(10)
                        .color(self.secondary_text_color())
                        .wrapping(iced::advanced::text::Wrapping::None)
                        .width(Length::FillPortion(3)),
                    default,
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                rows = rows.push(
                    button(content)
                        .on_press(Message::OpenWithSelected(application.id.clone()))
                        .padding(Padding::from([4, 6]))
                        .style(|theme, status| context_button_style(theme, status, false))
                        .width(Fill),
                );
            }
            scrollable(rows).height(Fill).into()
        };

        let feedback: Element<'_, Message> = if error.is_empty() {
            text("Enter open")
                .font(MONO_FONT)
                .size(11)
                .color(self.secondary_text_color())
                .into()
        } else {
            text(error)
                .size(11)
                .color(self.iced_theme().palette().danger)
                .into()
        };
        let custom = row![
            text("custom")
                .font(MONO_FONT)
                .size(11)
                .color(self.accent_color()),
            text_input("Application name or desktop ID", custom)
                .id(Id::new(OPEN_WITH_ID))
                .on_input(Message::OpenWithChanged)
                .on_submit(Message::OpenWithSubmitted)
                .font(MONO_FONT)
                .size(12)
                .line_height(iced::Pixels(15.0))
                .padding(0)
                .style(status_input_style)
                .width(Fill),
            feedback,
        ]
        .spacing(7)
        .height(27)
        .align_y(Alignment::Center);

        container(column![header, options, custom].spacing(3))
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

    fn name_prompt_bar(
        self,
        label: &'a str,
        value: &'a str,
        error: &'a str,
    ) -> Element<'a, Message> {
        let feedback: Element<'_, Message> = if self.foreground_operation_active() {
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
                    .on_input_maybe(
                        (!self.foreground_operation_active())
                            .then_some(Message::PromptInputChanged),
                    )
                    .on_submit_maybe(
                        (!self.foreground_operation_active()).then_some(Message::PromptSubmit),
                    )
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

    fn search_count_view(self) -> Element<'a, Message> {
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

    pub(super) fn context_menu_view(self, menu: ContextMenu) -> Element<'a, Message> {
        let mut actions = Column::new();
        for (index, (label, message)) in self.context_actions(menu.target).into_iter().enumerate() {
            let focused = index == menu.focused;
            actions = actions.push(
                button(text(label).size(13))
                    .on_press(message)
                    .style(move |theme, status| context_button_style(theme, status, focused))
                    .width(Fill),
            );
        }
        let panel = container(scrollable(actions).height(Length::Shrink))
            .width(220)
            .max_height(420)
            .padding(5)
            .style(menu_style);
        let overlay =
            mouse_area(container("").width(Fill).height(Fill)).on_press(Message::CloseContext);
        stack![overlay, pin(panel).x(menu.point.x).y(menu.point.y)].into()
    }

    pub(super) fn accent_color(self) -> Color {
        if self.high_contrast() {
            return self.iced_theme().palette().primary;
        }
        self.accent
            .as_ref()
            .map_or(Color::from_rgb8(0, 120, 212), |colors| colors.accent)
    }

    pub(super) fn secondary_text_color(self) -> Color {
        let mut color = self.iced_theme().palette().text;
        color.a = if self.high_contrast() || self.reduced_transparency() {
            1.0
        } else {
            0.62
        };
        color
    }

    pub(super) fn selection_text_color(self) -> Color {
        if self.high_contrast() {
            return Color::BLACK;
        }
        self.accent
            .as_ref()
            .and_then(|colors| colors.selection_foreground)
            .unwrap_or(self.iced_theme().palette().text)
    }

    pub(super) fn entry_icon_color(self, kind: EntryIconKind) -> Color {
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
