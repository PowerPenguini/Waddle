//! Preserve Unix path bytes while keeping existing UTF-8 JSON paths readable.

use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serializer, ser::SerializeStruct};

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredPath {
    Text(String),
    Bytes { unix_bytes: Vec<u8> },
}

impl StoredPath {
    fn into_path(self) -> PathBuf {
        match self {
            Self::Text(text) => PathBuf::from(text),
            Self::Bytes { unix_bytes } => PathBuf::from(OsString::from_vec(unix_bytes)),
        }
    }
}

pub(crate) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    if let Some(text) = path.to_str() {
        serializer.serialize_str(text)
    } else {
        let mut stored = serializer.serialize_struct("UnixPath", 1)?;
        stored.serialize_field("unix_bytes", path.as_os_str().as_bytes())?;
        stored.end()
    }
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    StoredPath::deserialize(deserializer).map(StoredPath::into_path)
}

pub(crate) mod optional {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct BorrowedPath<'a>(#[serde(serialize_with = "super::serialize")] &'a Path);

    pub(crate) fn serialize<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        path.as_deref().map(BorrowedPath).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        Option::<StoredPath>::deserialize(deserializer).map(|path| path.map(StoredPath::into_path))
    }
}
