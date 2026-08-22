use super::grid::{DeleteMotion, Motion};

pub(super) const HELP: &str = "\
Browser
  [count]h/j/k/l Move across the file grid
  0 / $         Move to the start / end of the grid row
  gg / G        Jump to the first / last entry
  [count]G      Jump to an entry by display position
  H / M / L     Jump within the visible viewport
  Ctrl+D/U      Move down / up by half a page
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

#[derive(Clone, Debug, Eq, PartialEq)]
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
    Pending(String),
    InvalidSequence(String),
    Move(Motion, usize),
    DeleteMotion(DeleteMotion, usize),
    Activate,
    Parent,
}

#[derive(Clone, Debug, Default)]
pub(super) struct BrowserInput {
    mode: Mode,
    count: Option<usize>,
    g_pending: bool,
    delete_pending: Option<(usize, Option<usize>)>,
}

impl BrowserInput {
    pub(super) fn mode(&self) -> Mode {
        self.mode
    }

    pub(super) fn enter(&mut self, mode: Mode) {
        self.mode = mode;
        if mode != Mode::Browser {
            self.clear_sequence();
        }
    }

    pub(super) fn leave_mode(&mut self) {
        self.mode = Mode::Browser;
    }

    #[cfg(test)]
    pub(super) fn delete_pending(&self) -> bool {
        self.delete_pending.is_some()
    }

    pub(super) fn pending_sequence(&self) -> Option<String> {
        if let Some((operator_count, motion_count)) = self.delete_pending {
            let operator = if operator_count > 1 {
                operator_count.to_string()
            } else {
                String::new()
            };
            let motion = motion_count
                .map(|count| count.to_string())
                .unwrap_or_default();
            Some(format!("{operator}d{motion}"))
        } else if self.g_pending {
            Some(format!(
                "{}g",
                self.count
                    .map(|count| count.to_string())
                    .unwrap_or_default()
            ))
        } else {
            self.count.map(|count| count.to_string())
        }
    }

    fn pending_status(&self) -> String {
        let sequence = self.pending_sequence().unwrap_or_default();
        let expected = if self.delete_pending.is_some() {
            "awaiting motion: 0, $, h, j, k, l, or d"
        } else if self.g_pending {
            "awaiting g"
        } else {
            "awaiting motion or operator"
        };
        format!("{sequence}  •  {expected}")
    }

    fn clear_sequence(&mut self) {
        self.count = None;
        self.g_pending = false;
        self.delete_pending = None;
    }

    fn push_count(&mut self, digit: usize) {
        let count = self.count.unwrap_or_default();
        self.count = Some(count.saturating_mul(10).saturating_add(digit).min(10_000));
    }

    fn invalid_sequence(&mut self, key: &str) -> Intent {
        let sequence = format!("{}{key}", self.pending_sequence().unwrap_or_default());
        self.clear_sequence();
        Intent::InvalidSequence(format!("Invalid Browser sequence: {sequence}"))
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
            if self.pending_sequence().is_some() {
                self.clear_sequence();
                return Intent::Pending("Browser sequence cancelled".to_owned());
            }
            return Intent::None;
        }

        if context.busy {
            return Intent::None;
        }

        let text = press.text.as_deref();
        if press.control && !press.alt && !press.logo {
            let count = self.count.take().unwrap_or(1);
            self.g_pending = false;
            self.delete_pending = None;
            return match text.map(str::to_ascii_lowercase).as_deref() {
                Some("c") => Intent::Copy,
                Some("v") => Intent::Paste,
                Some("o") => Intent::Back,
                Some("d") => Intent::Move(Motion::HalfPageDown, count),
                Some("u") => Intent::Move(Motion::HalfPageUp, count),
                _ => Intent::None,
            };
        }

        if let Some(value) = text
            && value.len() == 1
            && let Some(digit) = value.chars().next().and_then(|value| value.to_digit(10))
            && (digit != 0 || self.count.is_some() || self.delete_pending.is_some())
        {
            if let Some((_, motion_count)) = self.delete_pending.as_mut() {
                let count = motion_count.unwrap_or_default();
                *motion_count = Some(
                    count
                        .saturating_mul(10)
                        .saturating_add(digit as usize)
                        .min(10_000),
                );
            } else if self.g_pending {
                return self.invalid_sequence(value);
            } else {
                self.push_count(digit as usize);
            }
            return Intent::Pending(self.pending_status());
        }

        if self.g_pending {
            if text == Some("g") {
                let count = self.count.take();
                self.g_pending = false;
                return Intent::Move(
                    count.map_or(Motion::First, |count| Motion::DisplayIndex(count - 1)),
                    1,
                );
            }
            return self.invalid_sequence(text.unwrap_or("key"));
        }

        if let Some((operator_count, motion_count)) = self.delete_pending.take() {
            let motion_count = motion_count.unwrap_or(1);
            return match text.and_then(delete_motion) {
                Some(motion) => Intent::DeleteMotion(
                    motion,
                    operator_count.saturating_mul(motion_count).min(10_000),
                ),
                None => self.invalid_sequence(text.unwrap_or("key")),
            };
        }

        let count = self.count.take();
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
                self.delete_pending = Some((count.unwrap_or(1), None));
                Intent::Pending(self.pending_status())
            }
            Some("g") => {
                self.count = count;
                self.g_pending = true;
                Intent::Pending(self.pending_status())
            }
            Some("G") => Intent::Move(
                count.map_or(Motion::Last, |count| Motion::DisplayIndex(count - 1)),
                1,
            ),
            Some("H") => Intent::Move(Motion::ViewportTop, count.unwrap_or(1)),
            Some("M") => Intent::Move(Motion::ViewportMiddle, count.unwrap_or(1)),
            Some("L") => Intent::Move(Motion::ViewportBottom, count.unwrap_or(1)),
            Some("h") => Intent::Move(Motion::Left, count.unwrap_or(1)),
            Some("j") => Intent::Move(Motion::Down, count.unwrap_or(1)),
            Some("k") => Intent::Move(Motion::Up, count.unwrap_or(1)),
            Some("l") => Intent::Move(Motion::Right, count.unwrap_or(1)),
            Some("0") => Intent::Move(Motion::RowStart, 1),
            Some("$") => Intent::Move(Motion::RowEnd, 1),
            _ if press.named == NamedKey::Enter => Intent::Activate,
            _ if press.named == NamedKey::Backspace => Intent::Parent,
            _ if press.named == NamedKey::Delete => Intent::Trash,
            _ if count.is_some() => {
                self.count = count;
                self.invalid_sequence(text.unwrap_or("key"))
            }
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

        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert!(input.delete_pending());
        assert_eq!(
            input.handle(text("$"), selected()),
            Intent::DeleteMotion(DeleteMotion::Motion(Motion::RowEnd), 1)
        );
        assert!(!input.delete_pending());

        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert!(matches!(
            input.handle(text("w"), selected()),
            Intent::InvalidSequence(_)
        ));
        assert!(!input.delete_pending());

        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                selected()
            ),
            Intent::Pending("Browser sequence cancelled".to_owned())
        );
        assert!(!input.delete_pending());
    }

    #[test]
    fn standalone_and_delete_motions_share_one_vocabulary() {
        let mut input = BrowserInput::default();

        assert_eq!(
            input.handle(text("0"), selected()),
            Intent::Move(Motion::RowStart, 1)
        );
        assert_eq!(
            input.handle(text("$"), selected()),
            Intent::Move(Motion::RowEnd, 1)
        );
        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("d"), selected()),
            Intent::DeleteMotion(DeleteMotion::Current, 1)
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
        assert_eq!(
            input.handle(control("d"), selected()),
            Intent::Move(Motion::HalfPageDown, 1)
        );
    }

    #[test]
    fn counts_and_g_sequences_compose_into_deterministic_motions() {
        let mut input = BrowserInput::default();

        assert!(matches!(
            input.handle(text("3"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("j"), selected()),
            Intent::Move(Motion::Down, 3)
        );
        assert!(matches!(
            input.handle(text("5"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("G"), selected()),
            Intent::Move(Motion::DisplayIndex(4), 1)
        );
        assert!(matches!(
            input.handle(text("g"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("g"), selected()),
            Intent::Move(Motion::First, 1)
        );
        assert_eq!(
            input.handle(text("G"), selected()),
            Intent::Move(Motion::Last, 1)
        );
    }

    #[test]
    fn viewport_and_half_page_motions_accept_counts() {
        let mut input = BrowserInput::default();

        assert_eq!(
            input.handle(text("H"), selected()),
            Intent::Move(Motion::ViewportTop, 1)
        );
        assert_eq!(
            input.handle(text("M"), selected()),
            Intent::Move(Motion::ViewportMiddle, 1)
        );
        assert_eq!(
            input.handle(text("L"), selected()),
            Intent::Move(Motion::ViewportBottom, 1)
        );

        assert!(matches!(
            input.handle(text("2"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(
                Press {
                    text: Some("d".to_owned()),
                    control: true,
                    ..Press::default()
                },
                selected()
            ),
            Intent::Move(Motion::HalfPageDown, 2)
        );
    }

    #[test]
    fn pending_sequences_wait_for_escape_and_invalid_input_explains_the_reset() {
        let mut input = BrowserInput::default();

        assert_eq!(
            input.handle(text("3"), selected()),
            Intent::Pending("3  •  awaiting motion or operator".to_owned())
        );
        assert_eq!(
            input.handle(text("q"), selected()),
            Intent::InvalidSequence("Invalid Browser sequence: 3q".to_owned())
        );
        assert_eq!(input.pending_sequence(), None);

        assert!(matches!(
            input.handle(text("g"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                selected()
            ),
            Intent::Pending("Browser sequence cancelled".to_owned())
        );
        assert_eq!(input.pending_sequence(), None);
    }

    #[test]
    fn counts_compose_before_and_after_the_delete_operator() {
        let mut input = BrowserInput::default();

        assert!(matches!(
            input.handle(text("3"), selected()),
            Intent::Pending(_)
        ));
        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("d"), selected()),
            Intent::DeleteMotion(DeleteMotion::Current, 3)
        );

        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert!(matches!(
            input.handle(text("3"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("j"), selected()),
            Intent::DeleteMotion(DeleteMotion::Motion(Motion::Down), 3)
        );

        assert!(matches!(
            input.handle(text("3"), selected()),
            Intent::Pending(_)
        ));
        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert!(matches!(
            input.handle(text("2"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("j"), selected()),
            Intent::DeleteMotion(DeleteMotion::Motion(Motion::Down), 6)
        );
    }
}
