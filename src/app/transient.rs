//! Presentation precedence and interaction policy shared by rendering, focus,
//! and keyboard routing. Domain sessions retain their own work and data.

use std::path::PathBuf;

use crate::fs::FileEntry;

use super::{
    browser_input::{BottomInput, BrowserInput, Context, Intent, Mode, Press},
    command::{CommandAction, CommandSession},
    file_operation::{self, FileOperationSession, PromptInteraction, View as FileOperationView},
    open_with,
    presentation::expanded_bar_height,
};

#[derive(Clone, Copy)]
pub(super) enum Dismiss {
    Command,
    CommandOutput,
    OpenWith,
    FileOperation,
}

/// A scoped edit of the participating sessions. Callers describe an interaction;
/// this module owns the accompanying cancellation and Browser key grammar mode.
/// The App adapter synchronizes presentation once the whole edit is complete.
pub(super) struct Sessions<'a> {
    command: &'a mut CommandSession,
    file_operation: &'a mut FileOperationSession,
    open_with: &'a mut open_with::Session,
    browser: &'a mut BrowserInput,
}

impl<'a> Sessions<'a> {
    pub(super) fn new(
        command: &'a mut CommandSession,
        file_operation: &'a mut FileOperationSession,
        open_with: &'a mut open_with::Session,
        browser: &'a mut BrowserInput,
    ) -> Self {
        Self {
            command,
            file_operation,
            open_with,
            browser,
        }
    }

    pub(super) fn begin_command(&mut self, prefix: char) {
        self.open_with.cancel();
        self.browser.enter(Mode::Command);
        self.command.begin(prefix);
    }

    pub(super) fn submit_command(&mut self, current: PathBuf) -> CommandAction {
        if self.browser.mode() != Mode::Command {
            return CommandAction::None;
        }
        self.browser.leave_mode();
        self.command.submit(current)
    }

    pub(super) fn begin_search(&mut self) {
        self.browser.enter(Mode::Search);
        self.command.close_output();
    }

    pub(super) fn begin_rename(&mut self, entry: FileEntry) {
        self.file_operation.begin_rename(entry);
        self.browser.enter(Mode::Rename);
        self.command.close_output();
    }

    pub(super) fn open_file_operation(&mut self, open: impl FnOnce(&mut FileOperationSession)) {
        if self.browser.mode() == Mode::Rename {
            self.dismiss(Dismiss::FileOperation);
        }
        self.command.close_output();
        open(self.file_operation);
    }

    pub(super) fn begin_open_with(&mut self, path: PathBuf) -> Result<(), String> {
        self.command.close_output();
        self.open_with.begin(path)?;
        Ok(())
    }

    pub(super) fn submit_open_with(&mut self) -> Option<open_with::Request> {
        let request = self.open_with.submit()?;
        Some(request)
    }

    pub(super) fn dismiss(&mut self, target: Dismiss) {
        let mode = match target {
            Dismiss::Command => {
                self.command.cancel();
                Some(Mode::Command)
            }
            Dismiss::CommandOutput => {
                self.command.close_output();
                None
            }
            Dismiss::OpenWith => {
                self.open_with.cancel();
                Some(Mode::OpenWith)
            }
            Dismiss::FileOperation => {
                if !self.file_operation.cancel() {
                    return;
                }
                Some(Mode::Rename)
            }
        };
        if mode == Some(self.browser.mode()) {
            self.browser.leave_mode();
        }
    }

    pub(super) fn blocks_action(&mut self) -> bool {
        if self.open_with.is_open() {
            self.dismiss(Dismiss::OpenWith);
        }
        if !self.file_operation.prompt_active() {
            return false;
        }
        if self.file_operation.is_busy() {
            return true;
        }
        self.dismiss(Dismiss::FileOperation);
        false
    }

    pub(super) fn prepare_trash(&mut self) {
        self.dismiss(Dismiss::FileOperation);
        self.command.close_output();
    }

    pub(super) fn confirm_file_operation(
        &mut self,
        current: PathBuf,
    ) -> Option<file_operation::Work> {
        self.file_operation.confirm(current)
    }

    pub(super) fn complete_file_operation(
        &mut self,
        completion: file_operation::Completion,
    ) -> file_operation::CompletionEffects {
        let mut effects = self.file_operation.complete(completion);
        if let Some(detail) = effects.detail.take() {
            self.command.show_settings(detail);
        }
        if effects.renamed && self.browser.mode() == Mode::Rename {
            self.browser.leave_mode();
        }
        effects
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Kind {
    Conflict,
    OpenWith,
    CommandOutput,
    FileOperation,
    TransferHistory,
    #[default]
    Standard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InputTarget {
    Search,
    Command,
    Rename,
    NewName,
    OpenWith,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Input {
    pub(super) target: InputTarget,
    pub(super) empty: bool,
}

pub(super) struct Sources<'a> {
    pub(super) command: &'a CommandSession,
    pub(super) file_operation: &'a FileOperationSession,
    pub(super) open_with: &'a open_with::Session,
    pub(super) mode: Mode,
    pub(super) search_empty: bool,
    pub(super) conflict: bool,
    pub(super) history_open: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Resolved {
    kind: Kind,
    expanded_height: Option<f32>,
    mode: Mode,
    input: Option<Input>,
    prompt: PromptInteraction,
}

impl Sources<'_> {
    pub(super) fn resolve(self) -> Resolved {
        let mut resolved = Resolved {
            kind: Kind::Standard,
            expanded_height: None,
            mode: self.mode,
            input: None,
            prompt: PromptInteraction::Inactive,
        };
        if self.conflict {
            resolved.kind = Kind::Conflict;
        } else if let Some(height) = self.open_with.preferred_height() {
            resolved.kind = Kind::OpenWith;
            resolved.expanded_height = Some(height);
            if let open_with::View::Open {
                custom,
                editing: true,
                ..
            } = self.open_with.view()
            {
                resolved.input = Some(Input {
                    target: InputTarget::OpenWith,
                    empty: custom.is_empty(),
                });
            }
        } else if let Some(output) = self.command.output() {
            resolved.kind = Kind::CommandOutput;
            resolved.expanded_height = Some(expanded_bar_height(&output.detail));
        } else if self.file_operation.prompt_active() {
            resolved.kind = Kind::FileOperation;
            resolved.prompt = self.file_operation.prompt_interaction();
            resolved.expanded_height = self
                .file_operation
                .expanded_detail()
                .map(expanded_bar_height);
            if let FileOperationView::NewFolder { value, .. }
            | FileOperationView::NewFile { value, .. } = self.file_operation.view()
            {
                resolved.input = Some(Input {
                    target: InputTarget::NewName,
                    empty: value.is_empty(),
                });
            }
        } else if self.history_open && self.mode == Mode::Browser {
            resolved.kind = Kind::TransferHistory;
            resolved.expanded_height = Some(190.0);
        } else {
            resolved.input = match self.mode {
                Mode::Search => Some(Input {
                    target: InputTarget::Search,
                    empty: self.search_empty,
                }),
                Mode::Command => Some(Input {
                    target: InputTarget::Command,
                    empty: self.command.text().is_empty(),
                }),
                Mode::Rename => match self.file_operation.view() {
                    FileOperationView::Rename { value, .. } => Some(Input {
                        target: InputTarget::Rename,
                        empty: value.is_empty(),
                    }),
                    _ => None,
                },
                _ => None,
            };
        }
        // An overlay suspends the underlying interaction; dismissing it reveals
        // the same input and text instead of cancelling a hidden session.
        if resolved.kind != Kind::Standard {
            resolved.mode = match (resolved.kind, self.mode) {
                (Kind::OpenWith, _) => Mode::OpenWith,
                // Location is in the toolbar, so output does not hide its editor.
                (Kind::CommandOutput, Mode::Location) => Mode::Location,
                _ => Mode::Browser,
            };
        }
        resolved
    }
}

impl Resolved {
    pub(super) fn kind(self) -> Kind {
        self.kind
    }
    pub(super) fn expanded_height(self) -> Option<f32> {
        self.expanded_height
    }
    pub(super) fn mode(self) -> Mode {
        self.mode
    }
    pub(super) fn input(self) -> Option<Input> {
        self.input
    }
    pub(super) fn restores_status_after(self, previous: Kind) -> bool {
        previous != Kind::Standard && self.kind == Kind::Standard
    }

    pub(super) fn restores_input_after(self, previous: Self) -> bool {
        self.input.is_some()
            && self.input.map(|input| input.target) != previous.input.map(|input| input.target)
    }

    pub(super) fn handle(
        self,
        browser: &mut BrowserInput,
        press: Press,
        mut context: Context,
    ) -> Intent {
        context.transfer_conflict = self.kind == Kind::Conflict;
        context.transfer_history_open = self.kind == Kind::TransferHistory;
        context.command_output = self.kind == Kind::CommandOutput;
        context.prompt = self.prompt;
        context.bottom_input = self.input.map_or(BottomInput::Inactive, |input| {
            BottomInput::new(true, input.empty)
        });
        browser.handle_in_mode(press, context, self.mode)
    }
}
