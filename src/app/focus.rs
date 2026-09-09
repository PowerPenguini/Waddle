//! Browser focus owns transitions, editor ownership, and ordered Iced effects.
//! Transient presentation determines which temporary input is visible; browser
//! focus retains the surface to return to when that input closes.
use super::{
    App, COMMAND_ID, InputMode, LOCATION_ID, Message, NEW_FOLDER_ID, OPEN_WITH_ID, RENAME_ID,
    SEARCH_ID, TransientPresentation,
};
use iced::{
    Task,
    widget::{self, Id},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum BrowserFocus {
    Sidebar,
    #[default]
    Entries,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusDirection {
    Left,
    Down,
    Up,
    Right,
}

impl BrowserFocus {
    const ORDER: [Self; 2] = [Self::Sidebar, Self::Entries];

    fn moved(self, reverse: bool) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or(0);
        let next = if reverse {
            index.checked_sub(1).unwrap_or(Self::ORDER.len() - 1)
        } else {
            (index + 1) % Self::ORDER.len()
        };
        Self::ORDER[next]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::Entries => "files",
        }
    }

    fn moved_in(self, direction: FocusDirection, tree_visible: bool) -> Self {
        match (self, direction) {
            (Self::Sidebar, FocusDirection::Right) => Self::Entries,
            (Self::Entries, FocusDirection::Left) if tree_visible => Self::Sidebar,
            _ => self,
        }
    }
}

#[derive(Default)]
pub(super) struct FocusSession {
    browser: BrowserFocus,
    location_editing: bool,
    pending_unfocus: bool,
    generation: u64,
}

impl FocusSession {
    pub(super) fn browser(&self) -> BrowserFocus {
        self.browser
    }
    pub(super) fn is(&self, target: BrowserFocus) -> bool {
        self.browser == target
    }
    pub(super) fn label(&self) -> &'static str {
        self.browser.label()
    }
}

impl App {
    pub(super) fn focus_browser(&mut self, target: BrowserFocus) {
        let target = if target == BrowserFocus::Sidebar && !self.view_preferences.tree_visible() {
            BrowserFocus::Entries
        } else {
            target
        };
        if self.focus.browser != target {
            self.browser_input.clear_sequence();
        }
        if self.focus.location_editing {
            self.focus.location_editing = false;
            if self.browser_input.mode() == InputMode::Location {
                self.browser_input.leave_mode();
            }
            self.location_input = self.navigation.current().display().to_string();
            self.focus.pending_unfocus = true;
        }
        self.focus.generation += 1;
        self.focus.browser = target;
        if target == BrowserFocus::Sidebar {
            self.sidebar_tree
                .focus_current_or_first(self.navigation.current());
        }
    }

    pub(super) fn move_browser_focus(&mut self, reverse: bool) {
        let mut target = self.focus.browser.moved(reverse);
        if !self.view_preferences.tree_visible() && target == BrowserFocus::Sidebar {
            target = target.moved(reverse);
        }
        self.focus_browser(target);
        self.presentation
            .set_status(format!("Focus: {}", self.focus.label()));
    }

    pub(super) fn move_browser_focus_in(&mut self, direction: FocusDirection) {
        let target = self
            .focus
            .browser
            .moved_in(direction, self.view_preferences.tree_visible());
        self.focus_browser(target);
        self.presentation
            .set_status(format!("Focus: {}", self.focus.label()));
    }

    pub(super) fn reconcile_focus_visibility(&mut self) {
        if self.focus.is(BrowserFocus::Sidebar) && !self.view_preferences.tree_visible() {
            self.focus_browser(BrowserFocus::Entries);
        }
    }

    pub(super) fn focus_location(&mut self) -> Task<Message> {
        self.focus.generation += 1;
        self.focus.location_editing = true;
        self.browser_input.enter(InputMode::Location);
        widget::operation::focus(Id::new(LOCATION_ID))
    }

    pub(super) fn observe_location_focus(
        &mut self,
        generation: u64,
        focused: bool,
    ) -> Task<Message> {
        if generation != self.focus.generation {
            return Task::none();
        }
        if focused {
            if !self.focus.location_editing {
                self.location_input = self.navigation.current().display().to_string();
            }
            self.focus_location()
        } else {
            if self.focus.location_editing && self.browser_input.mode() == InputMode::Location {
                self.browser_input.leave_mode();
                self.location_input = self.navigation.current().display().to_string();
            }
            self.focus.location_editing = false;
            Task::none()
        }
    }

    pub(super) fn release_location_focus(&mut self) -> Task<Message> {
        self.focus.generation += 1;
        self.focus.location_editing = false;
        if self.browser_input.mode() == InputMode::Location {
            self.browser_input.leave_mode();
        }
        unfocus_widget()
    }

    pub(super) fn probe_location_focus(&mut self) -> Task<Message> {
        self.focus.generation += 1;
        let generation = self.focus.generation;
        widget::operation::is_focused(Id::new(LOCATION_ID)).map(move |focused| {
            Message::LocationFocusChanged {
                generation,
                focused,
            }
        })
    }

    pub(super) fn finish_focus_update(
        &mut self,
        previous: TransientPresentation,
        task: Task<Message>,
    ) -> Task<Message> {
        let unfocus = std::mem::take(&mut self.focus.pending_unfocus);
        let effect = if self.transient_presentation().restores_input_after(previous) {
            self.refocus_bottom_input()
        } else if unfocus {
            unfocus_widget()
        } else {
            Task::none()
        };
        // Restored focus precedes action-specific text selection (Rename).
        effect.chain(task)
    }

    fn active_bottom_input(&self) -> Option<(&'static str, bool)> {
        self.transient_presentation().input().map(|input| {
            use super::transient::InputTarget;
            let id = match input.target {
                InputTarget::Search => SEARCH_ID,
                InputTarget::Command => COMMAND_ID,
                InputTarget::Rename => RENAME_ID,
                InputTarget::NewName => NEW_FOLDER_ID,
                InputTarget::OpenWith => OPEN_WITH_ID,
            };
            (id, input.empty)
        })
    }

    pub(super) fn active_bottom_input_empty(&self) -> bool {
        self.active_bottom_input().is_some_and(|(_, empty)| empty)
    }

    pub(super) fn bottom_input_active(&self) -> bool {
        self.active_bottom_input().is_some()
    }

    pub(super) fn refocus_bottom_input(&mut self) -> Task<Message> {
        self.focus_bottom_input(false)
    }

    pub(super) fn focus_bottom_input(&mut self, select_all: bool) -> Task<Message> {
        self.active_bottom_input()
            .map_or_else(Task::none, |(id, _)| {
                self.focus.generation += 1;
                self.focus.location_editing = false;
                if self.browser_input.mode() == InputMode::Location {
                    self.browser_input.leave_mode();
                    self.location_input = self.navigation.current().display().to_string();
                }
                widget::operation::focus(Id::new(id)).chain(if select_all {
                    widget::operation::select_all(Id::new(id))
                } else {
                    Task::none()
                })
            })
    }
}

fn unfocus_widget() -> Task<Message> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
}
