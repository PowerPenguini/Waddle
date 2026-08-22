use super::grid::{DeleteMotion, Motion};

pub(super) const HELP: &str = "\
Browser
  h j k l       Move across the file grid
  0 / $         Move to the start / end of the grid row
  Enter         Open the selected item
  Backspace     Go to the parent directory
  u / Ctrl+O    Go back
  v             Toggle visual selection
  r             Rename the selected item
  x / Delete    Move the selection to Trash
  dd            Move the active item to Trash
  d{motion}     Delete with 0, $, h, j, k, or l
  y / n         Confirm / cancel a deletion prompt
  /query        Search the current directory
  //query       Search recursively
  n / N         Repeat search forward / backward
  y / p         Copy / paste
  Esc           Cancel the active mode or close output";

pub(super) const DELETE_PENDING_STATUS: &str = "d  •  awaiting motion: 0, $, h, j, k, l, or d";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Mode {
    #[default]
    Browser,
    Location,
    Search,
    Command,
    Rename,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum NamedKey {
    Escape,
    Enter,
    Backspace,
    Delete,
    #[default]
    Other,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Press {
    pub(super) text: Option<String>,
    pub(super) named: NamedKey,
    pub(super) control: bool,
    pub(super) alt: bool,
    pub(super) logo: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Context {
    pub(super) prompt_active: bool,
    pub(super) prompt_accepts_enter: bool,
    pub(super) prompt_uses_yes_no: bool,
    pub(super) busy: bool,
    pub(super) command_output: bool,
    pub(super) visual_active: bool,
    pub(super) selection_count: usize,
    pub(super) has_selection: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Intent {
    None,
    PromptCancel,
    PromptConfirm,
    CancelSearch,
    CancelCommand,
    CancelRename,
    CancelLocation,
    CloseCommandOutput,
    CancelVisual,
    Copy,
    Paste,
    Back,
    BeginSearch,
    BeginCommand(char),
    RepeatSearch(bool),
    Rename,
    ToggleVisual,
    Trash,
    ArmDelete,
    Move(Motion),
    DeleteMotion(DeleteMotion),
    Activate,
    Parent,
}

#[derive(Clone, Debug, Default)]
pub(super) struct BrowserInput {
    mode: Mode,
    delete_pending: bool,
}

impl BrowserInput {
    pub(super) fn mode(&self) -> Mode {
        self.mode
    }

    pub(super) fn enter(&mut self, mode: Mode) {
        self.mode = mode;
        if mode != Mode::Browser {
            self.delete_pending = false;
        }
    }

    pub(super) fn leave_mode(&mut self) {
        self.mode = Mode::Browser;
    }

    pub(super) fn delete_pending(&self) -> bool {
        self.delete_pending
    }

    pub(super) fn handle(&mut self, press: Press, context: Context) -> Intent {
        if context.prompt_active {
            let answer = press.text.as_deref().map(str::to_ascii_lowercase);
            return if context.busy {
                Intent::None
            } else if press.named == NamedKey::Escape {
                Intent::PromptCancel
            } else if context.prompt_uses_yes_no && answer.as_deref() == Some("y") {
                Intent::PromptConfirm
            } else if context.prompt_uses_yes_no && answer.as_deref() == Some("n") {
                Intent::PromptCancel
            } else if press.named == NamedKey::Enter && context.prompt_accepts_enter {
                Intent::PromptConfirm
            } else {
                Intent::None
            };
        }

        if self.mode != Mode::Browser {
            if press.named != NamedKey::Escape {
                return Intent::None;
            }
            return match self.mode {
                Mode::Search => {
                    self.leave_mode();
                    Intent::CancelSearch
                }
                Mode::Command => {
                    self.leave_mode();
                    Intent::CancelCommand
                }
                Mode::Rename if !context.busy => {
                    self.leave_mode();
                    Intent::CancelRename
                }
                Mode::Rename => Intent::None,
                Mode::Location => {
                    self.leave_mode();
                    Intent::CancelLocation
                }
                Mode::Browser => Intent::None,
            };
        }

        if press.named == NamedKey::Escape {
            if context.command_output {
                return Intent::CloseCommandOutput;
            }
            if context.visual_active {
                return Intent::CancelVisual;
            }
            if self.delete_pending {
                self.delete_pending = false;
            }
            return Intent::None;
        }

        if context.busy {
            return Intent::None;
        }

        let text = press.text.as_deref();
        if press.control && !press.alt && !press.logo {
            return match text.map(str::to_ascii_lowercase).as_deref() {
                Some("c") => Intent::Copy,
                Some("v") => Intent::Paste,
                Some("o") => Intent::Back,
                _ => Intent::None,
            };
        }

        if self.delete_pending {
            self.delete_pending = false;
            return text
                .and_then(delete_motion)
                .map_or(Intent::None, Intent::DeleteMotion);
        }

        match text {
            Some("/") => Intent::BeginSearch,
            Some("!") => Intent::BeginCommand('!'),
            Some(":") => Intent::BeginCommand(':'),
            Some("n") => Intent::RepeatSearch(false),
            Some("N") => Intent::RepeatSearch(true),
            Some("u") => Intent::Back,
            Some("r") => Intent::Rename,
            Some("y") => Intent::Copy,
            Some("p") => Intent::Paste,
            Some("v") => Intent::ToggleVisual,
            Some("x") => Intent::Trash,
            Some("d") if context.visual_active || context.selection_count > 1 => Intent::Trash,
            Some("d") if context.has_selection => {
                self.delete_pending = true;
                Intent::ArmDelete
            }
            Some("h") => Intent::Move(Motion::Left),
            Some("j") => Intent::Move(Motion::Down),
            Some("k") => Intent::Move(Motion::Up),
            Some("l") => Intent::Move(Motion::Right),
            Some("0") => Intent::Move(Motion::RowStart),
            Some("$") => Intent::Move(Motion::RowEnd),
            _ if press.named == NamedKey::Enter => Intent::Activate,
            _ if press.named == NamedKey::Backspace => Intent::Parent,
            _ if press.named == NamedKey::Delete => Intent::Trash,
            _ => Intent::None,
        }
    }
}

fn delete_motion(text: &str) -> Option<DeleteMotion> {
    match text {
        "d" => Some(DeleteMotion::Current),
        "h" => Some(DeleteMotion::Motion(Motion::Left)),
        "j" => Some(DeleteMotion::Motion(Motion::Down)),
        "k" => Some(DeleteMotion::Motion(Motion::Up)),
        "l" => Some(DeleteMotion::Motion(Motion::Right)),
        "0" => Some(DeleteMotion::Motion(Motion::RowStart)),
        "$" => Some(DeleteMotion::Motion(Motion::RowEnd)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Press {
        Press {
            text: Some(value.to_owned()),
            ..Press::default()
        }
    }

    fn selected() -> Context {
        Context {
            selection_count: 1,
            has_selection: true,
            ..Context::default()
        }
    }

    #[test]
    fn delete_operator_owns_valid_invalid_and_cancelled_sequences() {
        let mut input = BrowserInput::default();

        assert_eq!(input.handle(text("d"), selected()), Intent::ArmDelete);
        assert!(input.delete_pending());
        assert_eq!(
            input.handle(text("$"), selected()),
            Intent::DeleteMotion(DeleteMotion::Motion(Motion::RowEnd))
        );
        assert!(!input.delete_pending());

        assert_eq!(input.handle(text("d"), selected()), Intent::ArmDelete);
        assert_eq!(input.handle(text("w"), selected()), Intent::None);
        assert!(!input.delete_pending());

        assert_eq!(input.handle(text("d"), selected()), Intent::ArmDelete);
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                selected()
            ),
            Intent::None
        );
        assert!(!input.delete_pending());
    }

    #[test]
    fn standalone_and_delete_motions_share_one_vocabulary() {
        let mut input = BrowserInput::default();

        assert_eq!(
            input.handle(text("0"), selected()),
            Intent::Move(Motion::RowStart)
        );
        assert_eq!(
            input.handle(text("$"), selected()),
            Intent::Move(Motion::RowEnd)
        );
        assert_eq!(input.handle(text("d"), selected()), Intent::ArmDelete);
        assert_eq!(
            input.handle(text("d"), selected()),
            Intent::DeleteMotion(DeleteMotion::Current)
        );
    }

    #[test]
    fn prompt_and_mode_precedence_are_stateful_and_testable() {
        let mut input = BrowserInput::default();
        input.enter(Mode::Search);
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                Context::default()
            ),
            Intent::CancelSearch
        );
        assert_eq!(input.mode(), Mode::Browser);

        input.enter(Mode::Rename);
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                Context {
                    busy: true,
                    ..Context::default()
                }
            ),
            Intent::None
        );
        assert_eq!(input.mode(), Mode::Rename);

        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                Context {
                    prompt_active: true,
                    ..Context::default()
                }
            ),
            Intent::PromptCancel
        );
    }

    #[test]
    fn deletion_prompts_accept_yes_no_and_keep_enter_escape_aliases() {
        let context = Context {
            prompt_active: true,
            prompt_accepts_enter: true,
            prompt_uses_yes_no: true,
            ..Context::default()
        };
        let mut input = BrowserInput::default();

        for answer in ["y", "Y"] {
            assert_eq!(input.handle(text(answer), context), Intent::PromptConfirm);
        }
        for answer in ["n", "N"] {
            assert_eq!(input.handle(text(answer), context), Intent::PromptCancel);
        }
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Enter,
                    ..Press::default()
                },
                context
            ),
            Intent::PromptConfirm
        );
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                context
            ),
            Intent::PromptCancel
        );
    }

    #[test]
    fn control_shortcuts_ignore_plain_browser_grammar() {
        let mut input = BrowserInput::default();
        let control = |value: &str| Press {
            text: Some(value.to_owned()),
            control: true,
            ..Press::default()
        };

        assert_eq!(input.handle(control("c"), selected()), Intent::Copy);
        assert_eq!(input.handle(control("V"), selected()), Intent::Paste);
        assert_eq!(input.handle(control("o"), selected()), Intent::Back);
        assert_eq!(input.handle(control("d"), selected()), Intent::None);
    }
}
