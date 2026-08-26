use std::{path::PathBuf, time::Duration};

use iced::time::Instant;

const EXPAND_DELAY: Duration = Duration::from_millis(350);
const ENTER_DELAY: Duration = Duration::from_millis(1_100);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Target {
    Sidebar { id: u64, path: PathBuf },
    Folder(PathBuf),
}

impl Target {
    fn path(&self) -> &PathBuf {
        match self {
            Self::Sidebar { path, .. } | Self::Folder(path) => path,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Effect {
    Expand(u64),
    Enter(PathBuf),
}

#[derive(Clone, Debug, Default)]
pub(super) struct State {
    active: Option<Active>,
}

#[derive(Clone, Debug)]
struct Active {
    target: Target,
    started: Instant,
    expanded: bool,
}

impl State {
    pub(super) fn set(&mut self, target: Option<Target>, now: Instant) {
        if self.active.as_ref().map(|active| &active.target) == target.as_ref() {
            return;
        }
        self.active = target.map(|target| Active {
            target,
            started: now,
            expanded: false,
        });
    }

    pub(super) fn cancel(&mut self) {
        self.active = None;
    }

    #[cfg(test)]
    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn progress(&self, path: &std::path::Path, now: Instant) -> Option<f32> {
        let active = self
            .active
            .as_ref()
            .filter(|active| active.target.path() == path)?;
        Some(
            (now.duration_since(active.started).as_secs_f32() / ENTER_DELAY.as_secs_f32()).min(1.0),
        )
    }

    pub(super) fn tick(&mut self, now: Instant) -> Option<Effect> {
        let active = self.active.as_mut()?;
        let elapsed = now.duration_since(active.started);
        if elapsed >= ENTER_DELAY {
            let path = active.target.path().clone();
            self.active = None;
            return Some(Effect::Enter(path));
        }
        if elapsed >= EXPAND_DELAY
            && !active.expanded
            && let Target::Sidebar { id, .. } = &active.target
        {
            active.expanded = true;
            return Some(Effect::Expand(*id));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_target_restarts_progress_and_moving_away_cancels() {
        let now = Instant::now();
        let mut state = State::default();
        state.set(Some(Target::Folder(PathBuf::from("/one"))), now);
        assert!(state.progress(std::path::Path::new("/one"), now).is_some());

        state.set(
            Some(Target::Folder(PathBuf::from("/two"))),
            now + Duration::from_millis(900),
        );
        assert_eq!(
            state.progress(
                std::path::Path::new("/two"),
                now + Duration::from_millis(900)
            ),
            Some(0.0)
        );
        state.set(None, now + Duration::from_millis(901));
        assert!(!state.is_active());
    }

    #[test]
    fn sidebar_expands_once_before_the_longer_enter_deadline() {
        let now = Instant::now();
        let mut state = State::default();
        state.set(
            Some(Target::Sidebar {
                id: 7,
                path: PathBuf::from("/folder"),
            }),
            now,
        );
        assert_eq!(state.tick(now + Duration::from_millis(349)), None);
        assert_eq!(
            state.tick(now + Duration::from_millis(350)),
            Some(Effect::Expand(7))
        );
        assert_eq!(state.tick(now + Duration::from_millis(700)), None);
        assert_eq!(
            state.tick(now + Duration::from_millis(1_100)),
            Some(Effect::Enter(PathBuf::from("/folder")))
        );
        assert!(!state.is_active());
    }
}
