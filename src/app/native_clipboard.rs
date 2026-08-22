use iced::{
    Subscription,
    window::{Window, raw_window_handle},
};

use crate::transfer::{
    ClipboardAdapter, ClipboardCompletion, ClipboardPayload, Event as TransferEvent,
};

use super::{native_dnd, x11_clipboard};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum Source {
    Wayland(native_dnd::Source),
    X11(x11_clipboard::Source),
}

#[derive(Clone, Debug)]
pub(super) struct Attached {
    pub(super) clipboard: Source,
    pub(super) wayland_dnd: Option<native_dnd::Source>,
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
                    wayland_dnd: Some(source),
                })
            }
            RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => Ok(Self {
                clipboard: Source::X11(x11_clipboard::Source::attach()?),
                wayland_dnd: None,
            }),
            _ => Err("native file clipboard is available on Wayland and X11".to_owned()),
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
