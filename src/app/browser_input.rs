use super::{
    file_operation::PromptInteraction,
    grid::{DeleteMotion, Motion},
};

pub(super) const HELP: &str = "\
Keyboard navigation
  Arrow keys  Move the active entry or focused control
  Shift+Arrow  Extend the conventional selection
  Home / End  Jump to the first / last entry
  Tab / Shift+Tab  Move focus forward / backward
  Enter  Open an entry or activate the focused control
  Space  Toggle selection or activate the focused control
  Backspace  Go to the parent directory
  F5  Refresh the current view
  Ctrl+L  Edit the current location
  Ctrl+O / Ctrl+I  Go back / forward
  Ctrl+W e  Toggle the sidebar tree

Vim navigation
  [count]h/j/k/l  Move across entries
  0 / $  Move to the start / end of the grid row
  gg / G  Jump to the first / last entry
  [count]gg / [count]G  Jump to a display position
  H / M / L  Jump to the viewport top / middle / bottom
  [count]Ctrl+D/U  Move down / up by half a page

Selection and file operations
  v  Toggle visual selection
  Ctrl+A  Select all entries
  y / Ctrl+C  Copy the selection
  p / Ctrl+V  Paste
  x  Cut the selection
  dd  Cut the active entry
  d{motion}  Cut through 0, $, h, j, k, l, or d
  \"_x / \"_dd / \"_d{motion}  Trash without changing clipboard
  Delete  Move the selection to Trash
  u / Ctrl+R  Undo / redo
  r  Rename the active entry

Search
  /query  Search the current directory
  //query  Search recursively
  n / N  Repeat search forward / backward
  Enter  Open the current match
  Esc  Cancel search and restore the previous view

Prompts and transfers
  y / Enter  Confirm a deletion prompt
  n / Esc  Cancel a deletion prompt
  Backspace  Cancel an empty bottom input
  r / s / k  Replace / skip / keep both for one conflict
  R / S / K  Apply the choice to remaining entries
  Esc  Cancel an active mode, sequence, transfer, or output

Pointer and list view
  Right click  Open file and folder actions
  Drag empty space  Select intersecting Grid entries
  Drag entries  Move them; hold Ctrl while dropping to copy
  Name/Type/Size/Modified header
    Sort by that property; click again to reverse";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Mode {
    #[default]
    Browser,
    Location,
    Search,
    Command,
    Rename,
    OpenWith,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum NamedKey {
    Escape,
    Enter,
    Backspace,
    Delete,
    Refresh,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    Space,
    Tab,
    #[default]
    Other,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Press {
    pub(super) text: Option<String>,
    pub(super) named: NamedKey,
    pub(super) control: bool,
    pub(super) shift: bool,
    pub(super) alt: bool,
    pub(super) logo: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum BottomInput {
    #[default]
    Inactive,
    Active {
        empty: bool,
    },
}

impl BottomInput {
    pub(super) fn new(active: bool, empty: bool) -> Self {
        if active {
            Self::Active { empty }
        } else {
            Self::Inactive
        }
    }

    fn is_empty(self) -> bool {
        matches!(self, Self::Active { empty: true })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Context {
    pub(super) transfer_conflict: bool,
    pub(super) prompt: PromptInteraction,
    pub(super) foreground_operation_active: bool,
    pub(super) command_output: bool,
    pub(super) visual_active: bool,
    pub(super) selection_count: usize,
    pub(super) has_selection: bool,
    pub(super) pending_cut: bool,
    pub(super) navigation_pending: bool,
    pub(super) file_operators_allowed: bool,
    pub(super) bottom_input: BottomInput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Intent {
    None,
    PromptCancel,
    PromptConfirm,
    ConflictCancel,
    ConflictChoice { key: char, remaining: bool },
    CancelSearch,
    CancelCommand,
    CancelRename,
    CancelOpenWith,
    CancelLocation,
    CloseCommandOutput,
    CopyCommandOutput,
    CancelVisual,
    CancelCut,
    CancelNavigation,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    Refresh,
    ToggleTree,
    BeginLocation,
    MoveFocus { reverse: bool },
    CompleteCommand,
    SelectAll,
    ToggleActive,
    StandardMove { motion: Motion, extend: bool },
    Back,
    Forward,
    BeginSearch,
    BeginCommand(char),
    RepeatSearch(bool),
    Rename,
    ToggleVisual,
    Trash,
    Pending(String),
    InvalidSequence(String),
    Move(Motion, usize),
    CutMotion(DeleteMotion, usize),
    TrashMotion(DeleteMotion, usize),
    Activate,
    Parent,
}

#[derive(Clone, Debug, Default)]
pub(super) struct BrowserInput {
    mode: Mode,
    count: Option<usize>,
    g_pending: bool,
    window_pending: bool,
    delete_pending: Option<(usize, Option<usize>, bool)>,
    black_hole_stage: u8,
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
        if let Some((operator_count, motion_count, black_hole)) = self.delete_pending {
            let operator = if operator_count > 1 {
                operator_count.to_string()
            } else {
                String::new()
            };
            let motion = motion_count
                .map(|count| count.to_string())
                .unwrap_or_default();
            Some(format!(
                "{}{operator}d{motion}",
                if black_hole { "\"_" } else { "" }
            ))
        } else if self.black_hole_stage > 0 {
            Some(if self.black_hole_stage == 1 {
                "\"".to_owned()
            } else {
                "\"_".to_owned()
            })
        } else if self.window_pending {
            Some("Ctrl+W".to_owned())
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
        } else if self.black_hole_stage == 1 {
            "awaiting _"
        } else if self.black_hole_stage == 2 {
            "awaiting d or x"
        } else if self.window_pending {
            "awaiting e"
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
        self.window_pending = false;
        self.delete_pending = None;
        self.black_hole_stage = 0;
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
        if context.transfer_conflict {
            if press.named == NamedKey::Escape {
                return Intent::ConflictCancel;
            }
            return match press.text.as_deref().and_then(|text| text.chars().next()) {
                Some(key @ ('r' | 's' | 'k'))
                    if press.text.as_deref().is_some_and(|v| v.len() == 1) =>
                {
                    Intent::ConflictChoice {
                        key,
                        remaining: false,
                    }
                }
                Some(key @ ('R' | 'S' | 'K'))
                    if press.text.as_deref().is_some_and(|v| v.len() == 1) =>
                {
                    Intent::ConflictChoice {
                        key: key.to_ascii_lowercase(),
                        remaining: true,
                    }
                }
                _ => Intent::None,
            };
        }
        if context.prompt.is_active() {
            let answer = press.text.as_deref().map(str::to_ascii_lowercase);
            return if context.foreground_operation_active {
                Intent::None
            } else if press.named == NamedKey::Escape
                || press.named == NamedKey::Backspace && context.bottom_input.is_empty()
            {
                Intent::PromptCancel
            } else if context.prompt.uses_yes_no() && answer.as_deref() == Some("y") {
                Intent::PromptConfirm
            } else if context.prompt.uses_yes_no() && answer.as_deref() == Some("n") {
                Intent::PromptCancel
            } else if press.named == NamedKey::Enter && context.prompt.accepts_enter() {
                Intent::PromptConfirm
            } else {
                Intent::None
            };
        }

        if self.mode != Mode::Browser {
            if self.mode == Mode::Command && press.named == NamedKey::Tab {
                return Intent::CompleteCommand;
            }
            let cancel = press.named == NamedKey::Escape
                || press.named == NamedKey::Backspace && context.bottom_input.is_empty();
            if !cancel {
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
                Mode::Rename if !context.foreground_operation_active => {
                    self.leave_mode();
                    Intent::CancelRename
                }
                Mode::Rename => Intent::None,
                Mode::OpenWith => {
                    self.leave_mode();
                    Intent::CancelOpenWith
                }
                Mode::Location => {
                    self.leave_mode();
                    Intent::CancelLocation
                }
                Mode::Browser => Intent::None,
            };
        }

        if context.command_output
            && !press.control
            && !press.alt
            && !press.logo
            && press
                .text
                .as_deref()
                .is_some_and(|text| text.eq_ignore_ascii_case("y"))
        {
            self.clear_sequence();
            return Intent::CopyCommandOutput;
        }

        if press.named == NamedKey::Escape {
            if context.command_output {
                return Intent::CloseCommandOutput;
            }
            if context.visual_active {
                return Intent::CancelVisual;
            }
            if context.pending_cut {
                return Intent::CancelCut;
            }
            if self.pending_sequence().is_some() {
                self.clear_sequence();
                return Intent::Pending("Browser sequence cancelled".to_owned());
            }
            if context.navigation_pending {
                return Intent::CancelNavigation;
            }
            return Intent::None;
        }

        if press.named == NamedKey::Refresh {
            self.clear_sequence();
            return Intent::Refresh;
        }

        if press.named == NamedKey::Tab {
            self.clear_sequence();
            return Intent::MoveFocus {
                reverse: press.shift,
            };
        }

        if context.foreground_operation_active {
            return Intent::None;
        }

        let text = press.text.as_deref();
        if self.window_pending {
            return if !press.control && !press.alt && !press.logo && text == Some("e") {
                self.clear_sequence();
                Intent::ToggleTree
            } else {
                self.invalid_sequence(text.unwrap_or("key"))
            };
        }
        if press.control && !press.alt && !press.logo {
            let count = self.count.take().unwrap_or(1);
            self.g_pending = false;
            self.window_pending = false;
            self.delete_pending = None;
            self.black_hole_stage = 0;
            return match text.map(str::to_ascii_lowercase).as_deref() {
                Some("c") => Intent::Copy,
                Some("a") => Intent::SelectAll,
                Some("l") => Intent::BeginLocation,
                Some("v") => Intent::Paste,
                Some("o") => Intent::Back,
                Some("i") => Intent::Forward,
                Some("r") => Intent::Redo,
                Some("d") => Intent::Move(Motion::HalfPageDown, count),
                Some("u") => Intent::Move(Motion::HalfPageUp, count),
                Some("w") => {
                    self.window_pending = true;
                    Intent::Pending(self.pending_status())
                }
                _ => Intent::None,
            };
        }

        let standard_motion = match press.named {
            NamedKey::ArrowLeft => Some(Motion::Left),
            NamedKey::ArrowRight => Some(Motion::Right),
            NamedKey::ArrowUp => Some(Motion::Up),
            NamedKey::ArrowDown => Some(Motion::Down),
            NamedKey::Home => Some(Motion::First),
            NamedKey::End => Some(Motion::Last),
            _ => None,
        };
        if let Some(motion) = standard_motion {
            return Intent::StandardMove {
                motion,
                extend: press.shift,
            };
        }
        if press.named == NamedKey::Space {
            return Intent::ToggleActive;
        }

        if let Some(value) = text
            && value.len() == 1
            && let Some(digit) = value.chars().next().and_then(|value| value.to_digit(10))
            && (digit != 0 || self.count.is_some() || self.delete_pending.is_some())
        {
            if let Some((_, motion_count, _)) = self.delete_pending.as_mut() {
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

        if self.black_hole_stage > 0 {
            return match (self.black_hole_stage, text) {
                (1, Some("_")) => {
                    self.black_hole_stage = 2;
                    Intent::Pending(self.pending_status())
                }
                (2, Some("x")) if context.file_operators_allowed => {
                    self.clear_sequence();
                    Intent::Trash
                }
                (2, Some("d")) if context.file_operators_allowed && context.has_selection => {
                    let count = self.count.take().unwrap_or(1);
                    self.black_hole_stage = 0;
                    self.delete_pending = Some((count, None, true));
                    Intent::Pending(self.pending_status())
                }
                (2, Some("x" | "d")) => {
                    self.clear_sequence();
                    Intent::InvalidSequence(
                        "File operators are unavailable in the focused sidebar".to_owned(),
                    )
                }
                _ => self.invalid_sequence(text.unwrap_or("key")),
            };
        }

        if let Some((operator_count, motion_count, black_hole)) = self.delete_pending.take() {
            let motion_count = motion_count.unwrap_or(1);
            return match text.and_then(delete_motion) {
                Some(motion) => {
                    let count = operator_count.saturating_mul(motion_count).min(10_000);
                    if black_hole {
                        Intent::TrashMotion(motion, count)
                    } else {
                        Intent::CutMotion(motion, count)
                    }
                }
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
            Some("u") => Intent::Undo,
            Some("r") => Intent::Rename,
            Some("y") if context.file_operators_allowed => Intent::Copy,
            Some("p") => Intent::Paste,
            Some("v") => Intent::ToggleVisual,
            Some("x") if context.file_operators_allowed => Intent::Cut,
            Some("d")
                if context.file_operators_allowed
                    && (context.visual_active || context.selection_count > 1) =>
            {
                Intent::Cut
            }
            Some("d") if context.file_operators_allowed && context.has_selection => {
                self.delete_pending = Some((count.unwrap_or(1), None, false));
                Intent::Pending(self.pending_status())
            }
            Some("\"") => {
                self.count = count;
                self.black_hole_stage = 1;
                Intent::Pending(self.pending_status())
            }
            Some("y" | "x" | "d") => Intent::InvalidSequence(
                "File operators are unavailable in the focused sidebar".to_owned(),
            ),
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
            _ if press.named == NamedKey::Delete && context.file_operators_allowed => Intent::Trash,
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
            file_operators_allowed: true,
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
            Intent::CutMotion(DeleteMotion::Motion(Motion::RowEnd), 1)
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
            Intent::CutMotion(DeleteMotion::Current, 1)
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

        input.enter(Mode::OpenWith);
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                Context::default()
            ),
            Intent::CancelOpenWith
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
                    foreground_operation_active: true,
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
                    prompt: PromptInteraction::Input,
                    ..Context::default()
                }
            ),
            Intent::PromptCancel
        );
    }

    #[test]
    fn plain_escape_cancels_pending_navigation_after_browser_state() {
        let mut input = BrowserInput::default();
        let escape = Press {
            named: NamedKey::Escape,
            ..Press::default()
        };

        assert_eq!(
            input.handle(
                escape.clone(),
                Context {
                    navigation_pending: true,
                    ..Context::default()
                }
            ),
            Intent::CancelNavigation
        );

        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert!(matches!(
            input.handle(
                escape,
                Context {
                    navigation_pending: true,
                    ..Context::default()
                }
            ),
            Intent::Pending(_)
        ));
    }

    #[test]
    fn backspace_closes_only_an_empty_bottom_input() {
        let mut input = BrowserInput::default();
        input.enter(Mode::Command);

        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Backspace,
                    ..Press::default()
                },
                Context::default()
            ),
            Intent::None
        );
        assert_eq!(input.mode(), Mode::Command);

        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Backspace,
                    ..Press::default()
                },
                Context {
                    bottom_input: BottomInput::Active { empty: true },
                    ..Context::default()
                }
            ),
            Intent::CancelCommand
        );
        assert_eq!(input.mode(), Mode::Browser);

        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Backspace,
                    ..Press::default()
                },
                Context {
                    prompt: PromptInteraction::Input,
                    bottom_input: BottomInput::Active { empty: true },
                    ..Context::default()
                }
            ),
            Intent::PromptCancel
        );
    }

    #[test]
    fn deletion_prompts_accept_yes_no_and_keep_enter_escape_aliases() {
        let context = Context {
            prompt: PromptInteraction::Confirmation,
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
    fn transfer_conflict_keys_take_precedence_and_preserve_uppercase_scope() {
        let context = Context {
            transfer_conflict: true,
            ..Context::default()
        };
        let mut input = BrowserInput::default();

        assert_eq!(
            input.handle(text("r"), context),
            Intent::ConflictChoice {
                key: 'r',
                remaining: false,
            }
        );
        assert_eq!(
            input.handle(text("K"), context),
            Intent::ConflictChoice {
                key: 'k',
                remaining: true,
            }
        );
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                context,
            ),
            Intent::ConflictCancel
        );
    }

    #[test]
    fn f5_requests_an_explicit_refresh() {
        let mut input = BrowserInput::default();
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Refresh,
                    ..Press::default()
                },
                selected(),
            ),
            Intent::Refresh
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
        assert_eq!(input.handle(control("i"), selected()), Intent::Forward);
        assert_eq!(input.handle(control("r"), selected()), Intent::Redo);
        assert_eq!(
            input.handle(control("l"), selected()),
            Intent::BeginLocation
        );
        assert_eq!(input.handle(text("u"), selected()), Intent::Undo);
        assert_eq!(
            input.handle(control("d"), selected()),
            Intent::Move(Motion::HalfPageDown, 1)
        );
    }

    #[test]
    fn command_output_uses_y_to_copy_and_escape_to_close() {
        let mut input = BrowserInput::default();
        let context = Context {
            command_output: true,
            ..selected()
        };

        assert_eq!(input.handle(text("y"), context), Intent::CopyCommandOutput);
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                context,
            ),
            Intent::CloseCommandOutput
        );
    }

    #[test]
    fn control_w_e_toggles_tree_and_uses_normal_sequence_rules() {
        let mut input = BrowserInput::default();
        let control_w = Press {
            text: Some("w".to_owned()),
            control: true,
            ..Press::default()
        };

        assert_eq!(
            input.handle(control_w.clone(), selected()),
            Intent::Pending("Ctrl+W  •  awaiting e".to_owned())
        );
        assert_eq!(input.handle(text("e"), selected()), Intent::ToggleTree);
        assert_eq!(input.pending_sequence(), None);

        assert!(matches!(
            input.handle(control_w.clone(), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("x"), selected()),
            Intent::InvalidSequence("Invalid Browser sequence: Ctrl+Wx".to_owned())
        );
        assert_eq!(input.pending_sequence(), None);

        assert!(matches!(
            input.handle(control_w, selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Escape,
                    ..Press::default()
                },
                selected(),
            ),
            Intent::Pending("Browser sequence cancelled".to_owned())
        );
    }

    #[test]
    fn tab_is_owned_by_command_completion_only_in_command_mode() {
        let mut input = BrowserInput::default();
        input.enter(Mode::Command);
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Tab,
                    ..Press::default()
                },
                selected(),
            ),
            Intent::CompleteCommand
        );
    }

    #[test]
    fn browser_tab_moves_composite_focus_in_both_directions() {
        let mut input = BrowserInput::default();
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Tab,
                    ..Press::default()
                },
                Context::default(),
            ),
            Intent::MoveFocus { reverse: false }
        );
        assert_eq!(
            input.handle(
                Press {
                    named: NamedKey::Tab,
                    shift: true,
                    ..Press::default()
                },
                Context::default(),
            ),
            Intent::MoveFocus { reverse: true }
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
            Intent::CutMotion(DeleteMotion::Current, 3)
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
            Intent::CutMotion(DeleteMotion::Motion(Motion::Down), 3)
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
            Intent::CutMotion(DeleteMotion::Motion(Motion::Down), 6)
        );
    }

    #[test]
    fn yank_is_single_key_while_cut_and_black_hole_trash_use_distinct_intents() {
        let mut input = BrowserInput::default();

        assert_eq!(input.handle(text("y"), selected()), Intent::Copy);
        assert_eq!(
            input.handle(text("j"), selected()),
            Intent::Move(Motion::Down, 1)
        );
        assert_eq!(input.handle(text("x"), selected()), Intent::Cut);

        assert!(matches!(
            input.handle(text("\""), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(input.pending_sequence().as_deref(), Some("\""));
        assert!(matches!(
            input.handle(text("_"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(input.pending_sequence().as_deref(), Some("\"_"));
        assert!(matches!(
            input.handle(text("d"), selected()),
            Intent::Pending(_)
        ));
        assert_eq!(
            input.handle(text("d"), selected()),
            Intent::TrashMotion(DeleteMotion::Current, 1)
        );
    }

    #[test]
    fn focused_sidebar_rejects_file_operators() {
        let mut input = BrowserInput::default();
        let context = Context {
            has_selection: true,
            selection_count: 1,
            file_operators_allowed: false,
            ..Context::default()
        };

        assert!(matches!(
            input.handle(text("d"), context),
            Intent::InvalidSequence(message) if message.contains("sidebar")
        ));
        assert!(matches!(
            input.handle(text("x"), context),
            Intent::InvalidSequence(message) if message.contains("sidebar")
        ));
    }
}
