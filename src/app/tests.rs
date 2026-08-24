use std::{fs as std_fs, path::PathBuf, time::Duration};

use iced::{event, keyboard, mouse};

use super::*;
use crate::app::file_operation::{
    Completion as FileOperationCompletion, View as FileOperationView,
};
use crate::app::grid::{
    CONTENT_GUTTER, LIST_HEADER_HEIGHT, LIST_ROW_HEIGHT, LIST_VIEW_TOP_INSET, Motion,
    SIDEBAR_WIDTH, TILE_ROW_HEIGHT, TOOLBAR_DIVIDER_HEIGHT, TOOLBAR_HEIGHT,
};
use crate::app::navigation::NavigationSession;
use crate::app::state::{ExplorerState, MountRoot, NodeKind};
use crate::fs::FileEntry;
use crate::transfer::{
    Action as TransferAction, Adapter as TransferAdapter, AdapterCompletion, ClipboardImport,
    Event as TransferEvent, Preview as TransferPreview, TransferState,
};

struct NoopTransferAdapter;

impl TransferAdapter for NoopTransferAdapter {
    fn start(
        &self,
        _paths: Vec<PathBuf>,
        _preview: TransferPreview,
        _copy_only: bool,
    ) -> Result<AdapterCompletion, String> {
        Err("unused test adapter".to_owned())
    }

    fn set_target(&self, _id: u64, _destination: Option<PathBuf>) {}

    fn finish_inbound(&self, _id: u64) {}

    fn shutdown(&self) {}
}

fn entry(name: &str) -> FileEntry {
    FileEntry {
        path: PathBuf::from("/start").join(name),
        name: name.into(),
        directory: false,
        metadata: Default::default(),
    }
}

fn press(app: &mut App, value: &'static str) {
    let key = keyboard::Key::Character(value.into());
    let _ = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some(value));
}

mod file_operation;
mod input;
mod navigation;
mod presentation;
mod transfer;
