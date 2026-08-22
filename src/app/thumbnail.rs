use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use iced::widget::image as widget_image;

const THUMBNAIL_EDGE: u32 = 96;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Fingerprint {
    length: u64,
    modified_nanos: u128,
}

impl Fingerprint {
    fn read(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        Some(Self {
            length: metadata.len(),
            modified_nanos: metadata
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_nanos(),
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct Request {
    path: PathBuf,
    fingerprint: Fingerprint,
}

#[derive(Clone, Debug)]
pub(super) struct Loaded {
    path: PathBuf,
    fingerprint: Fingerprint,
    result: Result<(u32, u32, Vec<u8>), String>,
}

#[derive(Clone)]
struct Cached {
    fingerprint: Fingerprint,
    handle: Option<widget_image::Handle>,
}

pub(super) struct Cache {
    capacity: usize,
    entries: HashMap<PathBuf, Cached>,
    pending: HashMap<PathBuf, Fingerprint>,
    lru: VecDeque<PathBuf>,
}

impl Default for Cache {
    fn default() -> Self {
        Self::new(128)
    }
}

impl Cache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            pending: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    pub(super) fn requests<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> Vec<Request> {
        let mut requests = Vec::new();
        for path in paths.into_iter().filter(|path| is_image(path)) {
            let Some(fingerprint) = Fingerprint::read(path) else {
                continue;
            };
            if self
                .entries
                .get(path)
                .is_some_and(|cached| cached.fingerprint == fingerprint)
            {
                self.touch(path);
                continue;
            }
            if self.pending.get(path) == Some(&fingerprint) {
                continue;
            }
            self.entries.remove(path);
            self.pending.insert(path.to_path_buf(), fingerprint);
            requests.push(Request {
                path: path.to_path_buf(),
                fingerprint,
            });
        }
        requests
    }

    pub(super) fn complete(&mut self, loaded: Loaded) {
        if self.pending.get(&loaded.path) != Some(&loaded.fingerprint) {
            return;
        }
        self.pending.remove(&loaded.path);
        let handle = loaded
            .result
            .ok()
            .map(|(width, height, pixels)| widget_image::Handle::from_rgba(width, height, pixels));
        self.entries.insert(
            loaded.path.clone(),
            Cached {
                fingerprint: loaded.fingerprint,
                handle,
            },
        );
        self.touch(&loaded.path);
        while self.entries.len() > self.capacity {
            if let Some(path) = self.lru.pop_front() {
                self.entries.remove(&path);
            }
        }
    }

    pub(super) fn handle(&self, path: &Path) -> Option<&widget_image::Handle> {
        self.entries.get(path)?.handle.as_ref()
    }

    fn touch(&mut self, path: &Path) {
        self.lru.retain(|candidate| candidate != path);
        self.lru.push_back(path.to_path_buf());
    }
}

pub(super) async fn load(request: Request) -> Loaded {
    let fallback = request.clone();
    tokio::task::spawn_blocking(move || decode(request))
        .await
        .unwrap_or_else(|error| Loaded {
            path: fallback.path,
            fingerprint: fallback.fingerprint,
            result: Err(error.to_string()),
        })
}

fn decode(request: Request) -> Loaded {
    let result = ::image::ImageReader::open(&request.path)
        .map_err(|error| error.to_string())
        .and_then(|reader| {
            reader
                .with_guessed_format()
                .map_err(|error| error.to_string())
        })
        .and_then(|reader| reader.decode().map_err(|error| error.to_string()))
        .and_then(|decoded| {
            if Fingerprint::read(&request.path) != Some(request.fingerprint) {
                return Err("image changed while its thumbnail was being generated".to_owned());
            }
            let thumbnail = decoded.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE).to_rgba8();
            let (width, height) = thumbnail.dimensions();
            Ok((width, height, thumbnail.into_raw()))
        });
    Loaded {
        path: request.path,
        fingerprint: request.fingerprint,
        result,
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "avif" | "bmp" | "gif" | "jpg" | "jpeg" | "png" | "webp"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(request: Request, value: u8) -> Loaded {
        Loaded {
            path: request.path,
            fingerprint: request.fingerprint,
            result: Ok((1, 1, vec![value, value, value, 255])),
        }
    }

    #[test]
    fn cache_is_lru_bounded_and_changed_files_are_requested_again() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ["one.png", "two.jpg", "three.webp"].map(|name| temp.path().join(name));
        for path in &paths {
            fs::write(path, "x").unwrap();
        }
        let mut cache = Cache::new(2);
        let requests = cache.requests(paths[..2].iter().map(PathBuf::as_path));
        cache.complete(loaded(requests[0].clone(), 1));
        cache.complete(loaded(requests[1].clone(), 2));
        assert!(cache.requests([paths[0].as_path()]).is_empty());
        let third = cache.requests([paths[2].as_path()]).remove(0);
        cache.complete(loaded(third, 3));
        assert!(cache.handle(&paths[0]).is_some());
        assert!(cache.handle(&paths[1]).is_none());

        fs::write(&paths[0], "changed length").unwrap();
        assert_eq!(cache.requests([paths[0].as_path()]).len(), 1);
    }

    #[test]
    fn only_supported_image_extensions_enter_the_work_queue() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("photo.PNG");
        let text = temp.path().join("notes.txt");
        fs::write(&image, "x").unwrap();
        fs::write(&text, "x").unwrap();

        let mut cache = Cache::new(2);
        let requests = cache.requests([image.as_path(), text.as_path()]);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, image);
    }
}
