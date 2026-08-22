use iced::{
    Subscription,
    window::{Window, raw_window_handle},
};

use crate::transfer::{
    Adapter, AdapterCompletion, ClipboardAdapter, ClipboardCompletion, ClipboardPayload,
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
    pub(super) clipboard: Source,
    pub(super) dnd: DndSource,
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
