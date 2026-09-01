use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use iced::widget::{image, svg};

use super::{EntryIconKind, tree};

#[derive(Clone, Debug)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Empty,
    Loading,
    Ready,
}

#[derive(Clone, Debug)]
pub(super) struct Load {
    generation: u64,
    theme: String,
}

#[derive(Clone, Debug)]
pub(super) struct Loaded {
    generation: u64,
    theme: String,
    cache: HashMap<Request, Option<Asset>>,
}

pub(super) struct Resolver {
    theme: String,
    enabled: bool,
    generation: u64,
    state: State,
    cache: HashMap<Request, Option<Asset>>,
}

impl Resolver {
    pub(super) fn new(theme: Option<String>, enabled: bool) -> (Self, Option<Load>) {
        let mut resolver = Self {
            theme: normalized_theme(theme),
            enabled,
            generation: 0,
            state: State::Empty,
            cache: HashMap::new(),
        };
        let load = resolver.begin_load();
        (resolver, load)
    }

    pub(super) fn configure(&mut self, theme: Option<String>, enabled: bool) -> Option<Load> {
        let theme = normalized_theme(theme);
        if self.theme != theme || self.enabled != enabled {
            self.theme = theme;
            self.enabled = enabled;
            self.generation = self.generation.wrapping_add(1);
            self.state = State::Empty;
            self.cache.clear();
        }
        self.begin_load()
    }

    pub(super) fn set_enabled(&mut self, enabled: bool) -> Option<Load> {
        self.configure(Some(self.theme.clone()), enabled)
    }

    pub(super) fn complete(&mut self, loaded: Loaded) -> bool {
        if !self.enabled
            || self.state != State::Loading
            || self.generation != loaded.generation
            || self.theme != loaded.theme
        {
            return false;
        }
        self.cache = loaded.cache;
        self.state = State::Ready;
        true
    }

    pub(super) fn resolve(&self, kind: Kind, size: u16) -> Option<Asset> {
        if !self.enabled || self.state != State::Ready {
            return None;
        }
        self.cache.get(&Request { kind, size }).cloned().flatten()
    }

    fn begin_load(&mut self) -> Option<Load> {
        if !self.enabled || self.state != State::Empty {
            return None;
        }
        self.state = State::Loading;
        Some(Load {
            generation: self.generation,
            theme: self.theme.clone(),
        })
    }
}

pub(super) async fn load(request: Load) -> Loaded {
    let fallback = request.clone();
    tokio::task::spawn_blocking(move || {
        load_with(request, |name, theme, size| {
            freedesktop_icons::lookup(name)
                .with_theme(theme)
                .with_size(size)
                .with_cache()
                .find()
        })
    })
    .await
    .unwrap_or_else(|_| Loaded {
        generation: fallback.generation,
        theme: fallback.theme,
        cache: HashMap::new(),
    })
}

fn load_with(load: Load, mut lookup: impl FnMut(&str, &str, u16) -> Option<PathBuf>) -> Loaded {
    let cache = requests()
        .into_iter()
        .map(|request| {
            let asset = names(request.kind).iter().find_map(|name| {
                lookup(name, &load.theme, request.size).and_then(|path| Asset::from_path(&path))
            });
            (request, asset)
        })
        .collect();
    Loaded {
        generation: load.generation,
        theme: load.theme,
        cache,
    }
}

fn requests() -> Vec<Request> {
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
    let mut requests = Vec::with_capacity(entry_kinds.len() * 2 + tree_kinds.len() + 1);
    for kind in entry_kinds {
        for size in [20, 48] {
            requests.push(Request {
                kind: Kind::Entry(kind),
                size,
            });
        }
    }
    requests.push(Request {
        kind: Kind::Entry(EntryIconKind::Folder),
        size: 44,
    });
    requests.extend(tree_kinds.map(|kind| Request {
        kind: Kind::Tree(kind),
        size: 17,
    }));
    requests
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
        assert!(
            requests()
                .into_iter()
                .all(|request| !names(request.kind).is_empty())
        );
    }

    #[test]
    fn one_configuration_starts_one_background_load() {
        let (mut resolver, first) = Resolver::new(Some("First".to_owned()), true);
        assert!(first.is_some());
        assert!(resolver.configure(Some("First".to_owned()), true).is_none());
        assert!(resolver.set_enabled(true).is_none());
    }

    #[test]
    fn completed_load_caches_every_request_and_stale_loads_are_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let icon = temp.path().join("icon.svg");
        std::fs::write(&icon, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        let calls = Cell::new(0);
        let (mut resolver, first) = Resolver::new(Some("First".to_owned()), true);
        let loaded = load_with(first.unwrap(), |_, _, _| {
            calls.set(calls.get() + 1);
            Some(icon.clone())
        });
        assert_eq!(calls.get(), requests().len());

        let second = resolver.configure(Some("Second".to_owned()), true).unwrap();
        assert!(!resolver.complete(loaded));
        assert!(
            resolver
                .resolve(Kind::Entry(EntryIconKind::Folder), 48)
                .is_none()
        );

        let loaded = load_with(second, |_, _, _| Some(icon.clone()));
        assert!(resolver.complete(loaded));
        for _ in 0..2 {
            assert!(
                resolver
                    .resolve(Kind::Entry(EntryIconKind::Folder), 48)
                    .is_some()
            );
        }
    }

    #[test]
    fn disabled_system_icons_wait_until_enabled_before_loading() {
        let (mut resolver, load) = Resolver::new(Some("First".to_owned()), false);
        assert!(load.is_none());
        assert!(resolver.set_enabled(true).is_some());
        assert!(resolver.set_enabled(true).is_none());
    }

    #[test]
    #[ignore = "release-mode performance benchmark"]
    fn benchmark_system_icon_cold_load_and_cached_resolution() {
        let started = std::time::Instant::now();
        let (mut resolver, load) = Resolver::new(Some("hicolor".to_owned()), true);
        let loaded = load_with(load.unwrap(), |name, theme, size| {
            freedesktop_icons::lookup(name)
                .with_theme(theme)
                .with_size(size)
                .with_cache()
                .find()
        });
        let cold = started.elapsed();
        assert!(resolver.complete(loaded));

        let started = std::time::Instant::now();
        for _ in 0..100_000 {
            std::hint::black_box(resolver.resolve(Kind::Entry(EntryIconKind::Folder), 48));
        }
        let warm = started.elapsed() / 100_000;
        println!(
            "benchmark system-icons: cold-load={cold:?} cached={warm:?}/op requests={}",
            requests().len()
        );
        assert!(
            cold <= std::time::Duration::from_secs(30),
            "system icon discovery exceeded its 30s background-work budget"
        );
        assert!(
            warm <= std::time::Duration::from_micros(2),
            "cached system icon resolution exceeded its 2us UI-thread budget"
        );
    }
}
