use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
};

use iced::widget::{image, svg};

use super::{EntryIconKind, tree};

#[derive(Clone)]
pub(super) enum Asset {
    Svg(svg::Handle),
    Raster(image::Handle),
}

impl Asset {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "svg" => Some(Self::Svg(svg::Handle::from_path(path))),
            "png" => Some(Self::Raster(image::Handle::from_path(path))),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Kind {
    Entry(EntryIconKind),
    Tree(tree::NodeKind),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Request {
    kind: Kind,
    size: u16,
}

pub(super) struct Resolver {
    theme: String,
    cache: RefCell<HashMap<Request, Option<Asset>>>,
}

impl Resolver {
    pub(super) fn new(theme: Option<String>) -> Self {
        Self {
            theme: normalized_theme(theme),
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub(super) fn set_theme(&mut self, theme: Option<String>) {
        let theme = normalized_theme(theme);
        if self.theme != theme {
            self.theme = theme;
            self.cache.get_mut().clear();
        }
    }

    pub(super) fn resolve(&self, kind: Kind, size: u16) -> Option<Asset> {
        self.resolve_with(kind, size, |name, theme, size| {
            freedesktop_icons::lookup(name)
                .with_theme(theme)
                .with_size(size)
                .find()
        })
    }

    fn resolve_with(
        &self,
        kind: Kind,
        size: u16,
        mut lookup: impl FnMut(&str, &str, u16) -> Option<PathBuf>,
    ) -> Option<Asset> {
        let request = Request { kind, size };
        if let Some(cached) = self.cache.borrow().get(&request) {
            return cached.clone();
        }

        let asset = names(kind).iter().find_map(|name| {
            lookup(name, &self.theme, size).and_then(|path| Asset::from_path(&path))
        });
        self.cache.borrow_mut().insert(request, asset.clone());
        asset
    }
}

fn normalized_theme(theme: Option<String>) -> String {
    theme
        .filter(|theme| !theme.trim().is_empty())
        .unwrap_or_else(|| "hicolor".to_owned())
}

fn names(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Entry(EntryIconKind::Folder) => &["folder"],
        Kind::Entry(EntryIconKind::Generic) => &["text-x-generic", "unknown"],
        Kind::Entry(EntryIconKind::Code) => &["text-x-script", "text-x-source"],
        Kind::Entry(EntryIconKind::Document) => &["x-office-document", "text-x-generic"],
        Kind::Entry(EntryIconKind::Pdf) => &["application-pdf", "x-office-document"],
        Kind::Entry(EntryIconKind::Image) => &["image-x-generic"],
        Kind::Entry(EntryIconKind::Audio) => &["audio-x-generic"],
        Kind::Entry(EntryIconKind::Video) => &["video-x-generic"],
        Kind::Entry(EntryIconKind::Archive) => &["package-x-generic", "application-x-archive"],
        Kind::Entry(EntryIconKind::Spreadsheet) => &["x-office-spreadsheet"],
        Kind::Entry(EntryIconKind::Presentation) => &["x-office-presentation"],
        Kind::Tree(tree::NodeKind::Computer) => &["computer"],
        Kind::Tree(tree::NodeKind::Drive) => &["drive-harddisk"],
        Kind::Tree(tree::NodeKind::Folder | tree::NodeKind::Favorite) => &["folder"],
        Kind::Tree(tree::NodeKind::Home) => &["user-home"],
        Kind::Tree(tree::NodeKind::Desktop) => &["user-desktop"],
        Kind::Tree(tree::NodeKind::Documents) => &["folder-documents", "folder"],
        Kind::Tree(tree::NodeKind::Downloads) => &["folder-download", "folder"],
        Kind::Tree(tree::NodeKind::Music) => &["folder-music", "folder"],
        Kind::Tree(tree::NodeKind::Pictures) => &["folder-pictures", "folder"],
        Kind::Tree(tree::NodeKind::Videos) => &["folder-videos", "folder"],
        Kind::Tree(tree::NodeKind::Recent) => &["document-open-recent"],
        Kind::Tree(tree::NodeKind::Trash) => &["user-trash"],
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn every_waddle_entry_and_tree_kind_has_a_system_name() {
        let entry_kinds = [
            EntryIconKind::Folder,
            EntryIconKind::Generic,
            EntryIconKind::Code,
            EntryIconKind::Document,
            EntryIconKind::Pdf,
            EntryIconKind::Image,
            EntryIconKind::Audio,
            EntryIconKind::Video,
            EntryIconKind::Archive,
            EntryIconKind::Spreadsheet,
            EntryIconKind::Presentation,
        ];
        let tree_kinds = [
            tree::NodeKind::Computer,
            tree::NodeKind::Drive,
            tree::NodeKind::Folder,
            tree::NodeKind::Home,
            tree::NodeKind::Desktop,
            tree::NodeKind::Documents,
            tree::NodeKind::Downloads,
            tree::NodeKind::Music,
            tree::NodeKind::Pictures,
            tree::NodeKind::Videos,
            tree::NodeKind::Favorite,
            tree::NodeKind::Recent,
            tree::NodeKind::Trash,
        ];

        assert!(
            entry_kinds
                .into_iter()
                .all(|kind| !names(Kind::Entry(kind)).is_empty())
        );
        assert!(
            tree_kinds
                .into_iter()
                .all(|kind| !names(Kind::Tree(kind)).is_empty())
        );
    }

    #[test]
    fn repeated_resolution_uses_the_local_cache_and_theme_changes_clear_it() {
        let temp = tempfile::tempdir().unwrap();
        let icon = temp.path().join("folder.svg");
        std::fs::write(&icon, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        let calls = Cell::new(0);
        let mut resolver = Resolver::new(Some("First".to_owned()));
        let kind = Kind::Entry(EntryIconKind::Folder);
        let lookup = |_: &str, _: &str, _: u16| {
            calls.set(calls.get() + 1);
            Some(icon.clone())
        };

        assert!(resolver.resolve_with(kind, 48, lookup).is_some());
        assert!(resolver.resolve_with(kind, 48, lookup).is_some());
        assert_eq!(calls.get(), 1);

        resolver.set_theme(Some("Second".to_owned()));
        assert!(resolver.resolve_with(kind, 48, lookup).is_some());
        assert_eq!(calls.get(), 2);
    }
}
