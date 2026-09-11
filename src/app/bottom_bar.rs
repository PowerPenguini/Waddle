use super::view::View;

#[cfg(test)]
#[path = "tests/open_with_layout.rs"]
mod open_with_layout;
use iced::{
    Alignment, Color, Element, Fill, Length, Padding,
    widget::{
        Column, Id, Row, Space, button, column, container, mouse_area, pin, row, scrollable, stack,
        text_input,
    },
};

use super::{
    BrowserStatusPresentation, COMMAND_ID, CONTENT_GUTTER, ContextMenu, EntryIconKind,
    FileOperationView, InputMode, Message, NEW_FOLDER_ID, OPEN_WITH_ID, RENAME_ID, SEARCH_ID,
    TransientPresentationKind, compact_status_line, context_menu_button_style,
    format_transfer_snapshot, menu_style, open_with, status_background_style, status_input_style,
    with_alpha,
};

pub(super) fn command_output_action_spacing() -> f32 {
    12.0
}

impl<'a> View<'a> {
    fn bottom_bar_text<'b>(
        self,
        value: impl iced::advanced::text::IntoFragment<'b>,
    ) -> iced::widget::Text<'b> {
        self.text(value).font(self.fonts().mono)
    }

    fn transfer_shortcut<'b>(self, label: &'b str, color: Color) -> Element<'b, Message> {
        self.bottom_bar_text(label).size(11).color(color).into()
    }
    pub(super) fn status_bar(self) -> Element<'a, Message> {
        let height = self.app().status_height();
        let status_model = self.app().browser_status_model();
        let resolved = self.app().transient_presentation();
        let transient = resolved.kind();
        let content: Element<'_, Message> = if transient == TransientPresentationKind::Conflict {
            compact_status_line(
                self.bottom_bar_text(status_model.text)
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
                self.bottom_bar_text(&output.summary)
                    .font(self.fonts().mono_semibold())
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .width(Fill),
                self.bottom_bar_text("y copy")
                    .size(11)
                    .color(self.secondary_text_color()),
                self.bottom_bar_text("Esc close")
                    .size(11)
                    .color(self.secondary_text_color()),
            ]
            .spacing(command_output_action_spacing())
            .height(29)
            .align_y(Alignment::Center);
            let output = column![
                header,
                scrollable(
                    self.bottom_bar_text(&output.detail)
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .color(with_alpha(self.app().iced_theme().palette().text, 0.84))
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
            let status: Element<'_, Message> = match resolved.mode() {
                InputMode::Search => {
                    let prefix = if self.app().search.is_recursive() {
                        "//"
                    } else {
                        "/"
                    };
                    row![
                        self.bottom_bar_text(prefix)
                            .size(12)
                            .line_height(iced::Pixels(15.0))
                            .color(self.accent_color()),
                        text_input("", self.app().search.query())
                            .id(Id::new(SEARCH_ID))
                            .on_input(Message::SearchChanged)
                            .on_submit(Message::SearchSubmitted)
                            .font(self.fonts().mono)
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
                    self.bottom_bar_text(self.app().command.prefix().unwrap_or(':').to_string())
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .color(self.accent_color()),
                    text_input("", self.app().command.text())
                        .id(Id::new(COMMAND_ID))
                        .on_input(Message::CommandChanged)
                        .on_submit(Message::CommandSubmitted)
                        .font(self.fonts().mono)
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
                        let feedback: Element<'_, Message> =
                            if self.app().foreground_operation_active() {
                                self.app().spinner(13.0).into()
                            } else if error.is_empty() {
                                self.bottom_bar_text("Enter save  ·  Esc cancel")
                                    .size(11)
                                    .line_height(iced::Pixels(13.0))
                                    .color(self.secondary_text_color())
                                    .into()
                            } else {
                                self.bottom_bar_text(error)
                                    .size(11)
                                    .line_height(iced::Pixels(13.0))
                                    .color(self.app().iced_theme().palette().danger)
                                    .into()
                            };
                        row![
                            self.bottom_bar_text("rename")
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(self.accent_color()),
                            text_input("", value)
                                .id(Id::new(RENAME_ID))
                                .on_input_maybe(
                                    (!self.app().foreground_operation_active())
                                        .then_some(Message::RenameChanged),
                                )
                                .on_submit_maybe(
                                    (!self.app().foreground_operation_active())
                                        .then_some(Message::RenameSubmitted),
                                )
                                .font(self.fonts().mono)
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
                                    self.app()
                                        .presentation
                                        .copy_feedback_intensity(self.app().reduced_motion()),
                                )
                            })
                            .into();
                    }
                    let indicator: Element<'_, Message> =
                        if self.app().foreground_operation_active()
                            || self.app().navigation.loading()
                        {
                            self.app().spinner(13.0).into()
                        } else {
                            Space::new().width(0).into()
                        };
                    let mut line = Row::new()
                        .push(indicator)
                        .push(
                            self.bottom_bar_text(status_model.text)
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(if self.app().presentation.notice_is_danger() {
                                    self.app().iced_theme().palette().danger
                                } else {
                                    self.secondary_text_color()
                                })
                                .width(Fill),
                        )
                        .spacing(
                            if self.app().foreground_operation_active()
                                || self.app().navigation.loading()
                            {
                                7
                            } else {
                                0
                            },
                        )
                        .align_y(Alignment::Center);
                    if status_model.retry {
                        line = line
                            .push(self.transfer_shortcut("R retry", self.secondary_text_color()));
                    }
                    if status_model.history {
                        line = line
                            .push(self.transfer_shortcut("t history", self.secondary_text_color()));
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
                    self.app()
                        .presentation
                        .copy_feedback_intensity(self.app().reduced_motion()),
                )
            })
            .into()
    }

    fn transfer_status_line(self) -> Element<'a, Message> {
        let transfers = self.app().transfers.overview();
        let mut line = Row::new().spacing(8).align_y(Alignment::Center);
        if let Some(snapshot) = transfers.snapshot {
            line = line
                .push(self.app().spinner(13.0))
                .push(
                    self.bottom_bar_text(format_transfer_snapshot(
                        transfers.active_action.unwrap_or("Transfer"),
                        &snapshot,
                    ))
                    .size(11)
                    .width(Fill),
                )
                .push(self.transfer_shortcut("Esc cancel", self.secondary_text_color()));
        } else {
            line = line.push(
                self.bottom_bar_text("Transfer finished with retained entries")
                    .size(11)
                    .width(Fill),
            );
        }
        if transfers.retry {
            line = line.push(self.transfer_shortcut("R retry", self.secondary_text_color()));
        }
        line.push(self.transfer_shortcut("t history", self.secondary_text_color()))
            .into()
    }

    fn transfer_history_bar(self) -> Element<'a, Message> {
        let transfers = self.app().transfers.overview();
        let mut header = Row::new()
            .push(
                self.bottom_bar_text("transfers")
                    .font(self.fonts().mono_semibold())
                    .size(11)
                    .width(Fill),
            )
            .spacing(10)
            .height(25)
            .align_y(Alignment::Center);
        if transfers.active {
            header = header.push(self.transfer_shortcut("c cancel", self.secondary_text_color()));
        }
        if transfers.retry {
            header = header.push(self.transfer_shortcut("R retry", self.secondary_text_color()));
        }
        header = header
            .push(self.transfer_shortcut("y copy", self.secondary_text_color()))
            .push(self.transfer_shortcut("Esc close", self.secondary_text_color()));
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
                    self.bottom_bar_text(detail)
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
            FileOperationView::PermanentDelete { message, detail } => {
                let header = row![
                    self.bottom_bar_text("delete permanently")
                        .font(self.fonts().mono_semibold())
                        .size(11)
                        .color(self.app().iced_theme().palette().danger),
                    self.bottom_bar_text(message).size(11).width(Fill),
                    self.bottom_bar_text("Y/n")
                        .font(self.fonts().mono_semibold())
                        .size(11)
                        .color(self.app().iced_theme().palette().danger),
                ]
                .spacing(8)
                .height(25)
                .align_y(Alignment::Center);
                container(
                    column![
                        header,
                        scrollable(
                            self.bottom_bar_text(detail)
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
            FileOperationView::Warning { message } => self.acknowledgement_bar(
                "warning",
                self.app().iced_theme().palette().warning,
                message,
            ),
            FileOperationView::Error { message } => {
                self.acknowledgement_bar("error", self.app().iced_theme().palette().danger, message)
            }
            FileOperationView::Idle | FileOperationView::Rename { .. } => Space::new().into(),
        }
    }

    fn acknowledgement_bar(
        self,
        label: &'static str,
        label_color: Color,
        message: &'a str,
    ) -> Element<'a, Message> {
        let header = row![
            self.bottom_bar_text(label)
                .font(self.fonts().mono_semibold())
                .size(11)
                .color(label_color),
            Space::new().width(Fill),
            self.bottom_bar_text("Esc close")
                .size(11)
                .color(self.secondary_text_color()),
        ]
        .height(25)
        .align_y(Alignment::Center);
        container(
            column![
                header,
                scrollable(
                    self.bottom_bar_text(message)
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

    fn open_with_bar(self) -> Element<'a, Message> {
        let open_with::View::Open {
            target_name,
            applications,
            selected,
            editing,
            custom,
            error,
        } = self.app().open_with.view()
        else {
            return Space::new().into();
        };

        let header = row![
            self.bottom_bar_text("open-with")
                .font(self.fonts().mono_semibold())
                .size(11)
                .color(self.accent_color()),
            self.bottom_bar_text(target_name)
                .size(11)
                .line_height(iced::Pixels(13.0))
                .width(Fill),
            self.bottom_bar_text(if editing { "Esc back" } else { "Esc cancel" })
                .size(11)
                .color(self.secondary_text_color()),
        ]
        .spacing(8)
        .height(25)
        .align_y(Alignment::Center);

        let mut rows = Column::new().spacing(1).width(Fill);
        if applications.is_empty() {
            rows = rows.push(
                container(
                    self.bottom_bar_text("No compatible applications found")
                        .size(11)
                        .color(self.secondary_text_color()),
                )
                .padding(Padding::from([4, 6])),
            );
        }
        // Keep the keyboard selection visible in the five-row viewport.
        let first = selected
            .saturating_sub(4)
            .min(applications.len().saturating_sub(5));
        for (index, application) in applications.iter().enumerate().skip(first).take(5) {
            let default: Element<'_, Message> = if application.default {
                self.bottom_bar_text("default")
                    .size(10)
                    .color(self.accent_color())
                    .into()
            } else {
                Space::new().width(0).into()
            };
            let content = row![
                self.bottom_bar_text(&application.name)
                    .size(12)
                    .line_height(iced::Pixels(14.0))
                    .width(Length::FillPortion(2)),
                self.bottom_bar_text(&application.id)
                    .size(10)
                    .color(self.secondary_text_color())
                    .wrapping(iced::advanced::text::Wrapping::None)
                    .width(Length::FillPortion(3)),
                // Reserve the badge column even when this is not the default app.
                container(default).width(60).align_x(Alignment::End),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            rows = rows.push(
                container(content)
                    .padding(Padding::from([4, 6]))
                    .width(Fill)
                    .style(move |theme: &iced::Theme| container::Style {
                        background: (selected == index)
                            .then(|| with_alpha(theme.palette().primary, 0.18).into()),
                        ..container::Style::default()
                    }),
            );
        }
        rows = rows.push(
            container(
                self.bottom_bar_text("Custom app...")
                    .size(12)
                    .line_height(iced::Pixels(14.0)),
            )
            .padding(Padding::from([4, 6]))
            .width(Fill)
            .style(move |theme: &iced::Theme| container::Style {
                background: (selected == applications.len())
                    .then(|| with_alpha(theme.palette().primary, 0.18).into()),
                ..container::Style::default()
            }),
        );
        let options = scrollable(rows).height(Fill);

        let feedback: Element<'_, Message> = if error.is_empty() {
            self.bottom_bar_text("Enter open")
                .size(11)
                .color(self.secondary_text_color())
                .into()
        } else {
            self.bottom_bar_text(error)
                .size(11)
                .color(self.app().iced_theme().palette().danger)
                .into()
        };
        let custom_content: Element<'_, Message> = if editing {
            row![
                self.bottom_bar_text("application")
                    .size(11)
                    .color(self.accent_color()),
                text_input("Application name, desktop ID, or executable path", custom)
                    .id(Id::new(OPEN_WITH_ID))
                    .on_input(Message::OpenWithChanged)
                    .on_submit(Message::OpenWithSubmitted)
                    .font(self.fonts().mono)
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
            row![
                Space::new().width(Fill),
                self.bottom_bar_text("j/k choose · Enter select")
                    .size(11)
                    .color(self.secondary_text_color()),
            ]
            .spacing(7)
            .align_y(Alignment::Center)
            .into()
        };
        let custom = container(custom_content)
            .padding(Padding::from([4, 6]))
            .height(27)
            .width(Fill);

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
        let feedback: Element<'_, Message> = if self.app().foreground_operation_active() {
            self.app().spinner(13.0).into()
        } else if error.is_empty() {
            self.bottom_bar_text("Enter create  ·  Esc cancel")
                .size(11)
                .color(self.secondary_text_color())
                .into()
        } else {
            self.bottom_bar_text(error)
                .size(11)
                .line_height(iced::Pixels(13.0))
                .color(self.app().iced_theme().palette().danger)
                .into()
        };
        compact_status_line(
            row![
                self.bottom_bar_text(label)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .color(self.accent_color()),
                text_input("", value)
                    .id(Id::new(NEW_FOLDER_ID))
                    .on_input_maybe(
                        (!self.app().foreground_operation_active())
                            .then_some(Message::PromptInputChanged),
                    )
                    .on_submit_maybe(
                        (!self.app().foreground_operation_active())
                            .then_some(Message::PromptSubmit),
                    )
                    .font(self.fonts().mono)
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
        if !self.app().search.is_recursive() || self.app().search.query().is_empty() {
            return Space::new().into();
        }
        if self.app().search.is_loading() {
            return row![Space::new().width(Fill), self.app().spinner(13.0)]
                .width(108)
                .align_y(Alignment::Center)
                .into();
        }
        let label = if self.app().search.is_truncated() {
            "1000+ matches".to_owned()
        } else {
            format!("{} matches", self.app().navigation.entries().len())
        };
        self.bottom_bar_text(label)
            .size(11)
            .line_height(iced::Pixels(13.0))
            .color(self.secondary_text_color())
            .into()
    }

    pub(super) fn context_menu_view(self, menu: ContextMenu) -> Element<'a, Message> {
        let mut actions = Column::new();
        for (index, (label, message)) in self
            .app()
            .context_actions(menu.target)
            .into_iter()
            .enumerate()
        {
            let focused = index == menu.focused;
            actions = actions.push(
                mouse_area(
                    button(self.text(label).size(13))
                        .on_press(message)
                        .style(move |theme, _| context_menu_button_style(theme, focused))
                        .width(Fill),
                )
                .on_enter(Message::ContextFocused(index)),
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
        if self.app().high_contrast() {
            return self.app().iced_theme().palette().primary;
        }
        self.app()
            .accent
            .as_ref()
            .map_or(Color::from_rgb8(0, 120, 212), |colors| colors.accent)
    }

    pub(super) fn secondary_text_color(self) -> Color {
        let mut color = self.app().iced_theme().palette().text;
        color.a = if self.app().high_contrast() || self.app().reduced_transparency() {
            1.0
        } else {
            0.62
        };
        color
    }

    pub(super) fn selection_text_color(self) -> Color {
        if self.app().high_contrast() {
            return Color::BLACK;
        }
        self.app()
            .accent
            .as_ref()
            .and_then(|colors| colors.selection_foreground)
            .unwrap_or(self.app().iced_theme().palette().text)
    }

    pub(super) fn entry_icon_color(self, kind: EntryIconKind) -> Color {
        let palette = self.app().iced_theme().palette();
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
