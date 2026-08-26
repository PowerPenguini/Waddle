use std::path::PathBuf;

use iced::{
    Subscription,
    window::{Window, raw_window_handle},
};

use crate::transfer::{
    Action, Adapter, AdapterCompletion, ClipboardAdapter, ClipboardCompletion, ClipboardPayload,
    Event as TransferEvent, Preview,
};

use super::{native_dnd, x11_clipboard, x11_dnd};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum Source {
    Wayland(native_dnd::Source),
    X11(x11_clipboard::Source),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum DndSource {
    Wayland(native_dnd::Source),
    X11(x11_dnd::Source),
}

#[derive(Clone, Debug)]
pub(super) struct Attached {
    clipboard: Source,
    dnd: DndSource,
}

#[derive(Default)]
pub(super) struct Platform {
    clipboard: Option<Source>,
    dnd: Option<DndSource>,
    dnd_error: Option<String>,
    x11_drop: X11Drop,
}

struct X11Drop {
    paths: Vec<PathBuf>,
    generation: u64,
    action: Action,
}

impl Default for X11Drop {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            generation: 0,
            action: Action::Copy,
        }
    }
}

impl Attached {
    pub(super) fn attach(window: &dyn Window) -> Result<Self, String> {
        use raw_window_handle::RawDisplayHandle;

        match window
            .display_handle()
            .map_err(|error| format!("could not access the display handle: {error}"))?
            .as_raw()
        {
            RawDisplayHandle::Wayland(_) => {
                let source = native_dnd::Source::attach(window)?;
                Ok(Self {
                    clipboard: Source::Wayland(source.clone()),
                    dnd: DndSource::Wayland(source),
                })
            }
            RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => {
                let clipboard = x11_clipboard::Source::attach()?;
                let dnd = x11_dnd::Source::attach(window)?;
                Ok(Self {
                    clipboard: Source::X11(clipboard),
                    dnd: DndSource::X11(dnd),
                })
            }
            _ => Err("native file clipboard is available on Wayland and X11".to_owned()),
        }
    }
}

impl Platform {
    pub(super) fn install(&mut self, result: Result<Attached, String>) -> Result<(), String> {
        match result {
            Ok(attached) => {
                self.clipboard = Some(attached.clipboard);
                self.dnd = Some(attached.dnd);
                self.dnd_error = None;
                Ok(())
            }
            Err(error) => {
                self.dnd_error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub(super) fn subscription(&self) -> Option<Subscription<TransferEvent>> {
        self.clipboard.as_ref().map(Source::subscription)
    }

    pub(super) fn clipboard(&self) -> Option<&Source> {
        self.clipboard.as_ref()
    }

    pub(super) fn dnd(&self) -> Option<&DndSource> {
        self.dnd.as_ref()
    }

    pub(super) fn take_dnd(&mut self) -> Option<DndSource> {
        self.dnd.take()
    }

    pub(super) fn dnd_error(&self) -> Option<&str> {
        self.dnd_error.as_deref()
    }

    pub(super) fn x11_active(&self) -> bool {
        self.dnd.as_ref().is_some_and(DndSource::is_x11)
    }

    pub(super) fn hover_x11_file(&mut self, path: PathBuf) -> Option<Action> {
        if !self.x11_active() {
            return None;
        }
        let action = self
            .dnd
            .as_ref()
            .map_or(Action::Copy, DndSource::incoming_action);
        Some(self.x11_drop.hover(path, action))
    }

    pub(super) fn leave_x11_files(&mut self) -> bool {
        if !self.x11_active() {
            return false;
        }
        self.x11_drop.leave();
        true
    }

    pub(super) fn drop_x11_file(&mut self, path: PathBuf) -> Option<u64> {
        if !self.x11_active() {
            return None;
        }
        Some(self.x11_drop.drop_path(path))
    }

    pub(super) fn take_x11_drop(&mut self, generation: u64) -> Option<(Vec<PathBuf>, Action)> {
        self.x11_drop.take(generation)
    }
}

impl X11Drop {
    fn hover(&mut self, path: PathBuf, action: Action) -> Action {
        let first_path = self.paths.is_empty();
        if !self.paths.contains(&path) {
            self.paths.push(path);
        }
        if first_path {
            self.action = action;
        }
        self.action
    }

    fn leave(&mut self) {
        self.paths.clear();
        self.action = Action::Copy;
    }

    fn drop_path(&mut self, path: PathBuf) -> u64 {
        if !self.paths.contains(&path) {
            self.paths.push(path);
        }
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    fn take(&mut self, generation: u64) -> Option<(Vec<PathBuf>, Action)> {
        if generation != self.generation || self.paths.is_empty() {
            return None;
        }
        let paths = std::mem::take(&mut self.paths);
        let action = std::mem::replace(&mut self.action, Action::Copy);
        Some((paths, action))
    }
}

impl DndSource {
    pub(super) fn is_x11(&self) -> bool {
        matches!(self, Self::X11(_))
    }

    pub(super) fn incoming_action(&self) -> crate::transfer::Action {
        match self {
            Self::Wayland(_) => crate::transfer::Action::Copy,
            Self::X11(source) => source.incoming_action(),
        }
    }
}

impl Adapter for DndSource {
    fn start(
        &self,
        paths: Vec<std::path::PathBuf>,
        preview: Preview,
        copy_only: bool,
    ) -> Result<AdapterCompletion, String> {
        match self {
            Self::Wayland(source) => Adapter::start(source, paths, preview, copy_only),
            Self::X11(source) => Adapter::start(source, paths, preview, copy_only),
        }
    }

    fn set_target(&self, id: u64, destination: Option<std::path::PathBuf>) {
        match self {
            Self::Wayland(source) => Adapter::set_target(source, id, destination),
            Self::X11(source) => Adapter::set_target(source, id, destination),
        }
    }

    fn finish_inbound(&self, id: u64) {
        match self {
            Self::Wayland(source) => Adapter::finish_inbound(source, id),
            Self::X11(source) => Adapter::finish_inbound(source, id),
        }
    }

    fn shutdown(&self) {
        match self {
            Self::Wayland(source) => Adapter::shutdown(source),
            Self::X11(source) => Adapter::shutdown(source),
        }
    }
}

impl Source {
    pub(super) fn subscription(&self) -> Subscription<TransferEvent> {
        match self {
            Self::Wayland(source) => source.subscription(),
            Self::X11(source) => source.subscription(),
        }
    }
}

impl ClipboardAdapter for Source {
    fn write_clipboard(&self, payload: ClipboardPayload) -> Result<(), String> {
        match self {
            Self::Wayland(source) => ClipboardAdapter::write_clipboard(source, payload),
            Self::X11(source) => ClipboardAdapter::write_clipboard(source, payload),
        }
    }

    fn read_clipboard(&self) -> Result<ClipboardCompletion, String> {
        match self {
            Self::Wayland(source) => ClipboardAdapter::read_clipboard(source),
            Self::X11(source) => ClipboardAdapter::read_clipboard(source),
        }
    }

    fn clear_clipboard(&self, generation: u64) {
        match self {
            Self::Wayland(source) => ClipboardAdapter::clear_clipboard(source, generation),
            Self::X11(source) => ClipboardAdapter::clear_clipboard(source, generation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_drop_staging_deduplicates_paths_and_rejects_stale_generations() {
        let mut drop = X11Drop::default();
        let first = PathBuf::from("/tmp/first");
        let second = PathBuf::from("/tmp/second");

        assert_eq!(drop.hover(first.clone(), Action::Move), Action::Move);
        assert_eq!(drop.hover(first.clone(), Action::Copy), Action::Move);
        let generation = drop.drop_path(second.clone());

        assert!(drop.take(generation.wrapping_sub(1)).is_none());
        let (paths, action) = drop.take(generation).unwrap();
        assert_eq!(paths, [first, second]);
        assert_eq!(action, Action::Move);
        assert!(drop.take(generation).is_none());
    }

    #[test]
    fn leaving_x11_drop_staging_resets_paths_and_action() {
        let mut drop = X11Drop::default();
        let _ = drop.hover(PathBuf::from("/tmp/first"), Action::Move);
        drop.leave();

        assert!(drop.paths.is_empty());
        assert_eq!(drop.action, Action::Copy);
    }
}
