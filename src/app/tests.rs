use std::{
    fs as std_fs,
    path::{Path, PathBuf},
};

use iced::{event, keyboard, mouse};

use super::*;
use crate::app::file_operation::View as FileOperationView;
use crate::app::grid::{
    CONTENT_GUTTER, LIST_HEADER_HEIGHT, LIST_ROW_HEIGHT, LIST_VIEW_TOP_INSET, Motion,
    SIDEBAR_WIDTH, TILE_ROW_HEIGHT, TOOLBAR_DIVIDER_HEIGHT, TOOLBAR_HEIGHT,
};
use crate::app::navigation::NavigationSession;
use crate::app::tree::{NodeKind, SidebarTree, VolumeRoot};
use crate::fs::FileEntry;
use crate::transfer::{
    Action as TransferAction, ClipboardImport, Event as TransferEvent, TransferState,
};

fn entry(name: &str) -> FileEntry {
    FileEntry {
        path: PathBuf::from("/start").join(name),
        name: name.into(),
        directory: false,
        metadata: Default::default(),
    }
}

fn opened(path: PathBuf, entries: Vec<FileEntry>) -> fs::OpenedDirectory {
    fs::OpenedDirectory {
        canonical_path: path,
        entries,
        child_folders: Vec::new(),
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
mod transient;
