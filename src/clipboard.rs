use std::path::PathBuf;

use gio::prelude::FileExt;

use crate::transfer::{Action, ClipboardImport, ClipboardPayload};

pub(crate) const URI_LIST_MIME: &str = "text/uri-list";
pub(crate) const GNOME_MIME: &str = "x-special/gnome-copied-files";
pub(crate) const KDE_CUT_MIME: &str = "application/x-kde-cutselection";
pub(crate) const POLAREXP_MIME: &str = "application/x-polarexp-file-list";
pub(crate) const MAX_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Debug)]
pub(crate) struct EncodedOffer {
    entries: Vec<(&'static str, Vec<u8>)>,
}

impl EncodedOffer {
    pub(crate) fn mime_types(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|(mime, _)| *mime)
    }

    pub(crate) fn data(&self, mime: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find_map(|(candidate, data)| (*candidate == mime).then_some(data.as_slice()))
    }

    pub(crate) fn into_entries(self) -> Vec<(&'static str, Vec<u8>)> {
        self.entries
    }
}

pub(crate) fn encode(payload: &ClipboardPayload) -> Result<EncodedOffer, String> {
    validate_entry_count(payload.paths.len())?;
    let uris = encode_uris(&payload.paths)?;
    let action = match payload.action {
        Action::Copy => "copy",
        Action::Move => "cut",
    };
    let uri_list = uris
        .iter()
        .map(|uri| format!("{uri}\r\n"))
        .collect::<String>();
    let gnome = format!(
        "{action}\n{}",
        uris.iter()
            .map(|uri| format!("{uri}\n"))
            .collect::<String>()
    );
    let private = format!(
        "polarexp-v1\ngeneration={}\naction={action}\n{}",
        payload.generation,
        uris.iter()
            .map(|uri| format!("{uri}\n"))
            .collect::<String>()
    );
    let entries = vec![
        (POLAREXP_MIME, private.into_bytes()),
        (GNOME_MIME, gnome.into_bytes()),
        (URI_LIST_MIME, uri_list.into_bytes()),
        (
            KDE_CUT_MIME,
            match payload.action {
                Action::Copy => b"0".to_vec(),
                Action::Move => b"1".to_vec(),
            },
        ),
    ];
    if entries.iter().any(|(_, data)| data.len() > MAX_BYTES) {
        return Err("the clipboard payload is larger than 4 MiB".to_owned());
    }
    Ok(EncodedOffer { entries })
}

pub(crate) fn decode(mime: &str, data: &[u8]) -> Result<ClipboardImport, String> {
    if data.len() > MAX_BYTES {
        return Err("the clipboard payload is larger than 4 MiB".to_owned());
    }
    let text = std::str::from_utf8(data)
        .map_err(|error| format!("the clipboard payload is not UTF-8: {error}"))?;
    match mime {
        URI_LIST_MIME => Ok(ClipboardImport {
            paths: decode_uri_lines(text.lines())?,
            action: Action::Copy,
            generation: None,
        }),
        GNOME_MIME => {
            let mut lines = text.lines();
            let action = match lines.next().map(str::trim) {
                Some("copy") => Action::Copy,
                Some("cut") => Action::Move,
                _ => Action::Copy,
            };
            Ok(ClipboardImport {
                paths: decode_uri_lines(lines)?,
                action,
                generation: None,
            })
        }
        POLAREXP_MIME => decode_private(text),
        _ => Err(format!("unsupported clipboard format: {mime}")),
    }
}

pub(crate) fn decode_offer(entries: &[(&str, &[u8])]) -> Result<ClipboardImport, String> {
    let mut imports = Vec::new();
    let mut markers = Vec::new();
    let mut malformed_marker = false;

    for &(mime, data) in entries {
        match mime {
            GNOME_MIME if data.is_empty() => {}
            POLAREXP_MIME | GNOME_MIME | URI_LIST_MIME => match decode(mime, data) {
                Ok(import) => {
                    if mime != URI_LIST_MIME {
                        markers.push(import.action);
                    }
                    imports.push(import);
                }
                Err(_) if mime == POLAREXP_MIME || mime == GNOME_MIME => {
                    malformed_marker = true;
                }
                Err(error) => return Err(error),
            },
            KDE_CUT_MIME => match data.strip_suffix(b"\0").unwrap_or(data).trim_ascii_end() {
                b"" => {}
                b"1" => markers.push(Action::Move),
                b"0" => markers.push(Action::Copy),
                _ => malformed_marker = true,
            },
            _ => {}
        }
    }

    let Some(primary) = imports.first() else {
        return Err("the clipboard does not contain local file paths".to_owned());
    };
    if imports.iter().any(|import| import.paths != primary.paths) {
        return Err("the clipboard formats contain different file lists".to_owned());
    }
    let action = if !malformed_marker
        && !markers.is_empty()
        && markers.iter().all(|action| *action == Action::Move)
    {
        Action::Move
    } else {
        Action::Copy
    };
    let generation = imports.iter().find_map(|import| import.generation);
    Ok(ClipboardImport {
        paths: primary.paths.clone(),
        action,
        generation,
    })
}

fn encode_uris(paths: &[PathBuf]) -> Result<Vec<String>, String> {
    paths
        .iter()
        .map(|path| {
            if !path.is_absolute() {
                return Err(format!("cannot copy a relative path: {}", path.display()));
            }
            Ok(gio::File::for_path(path).uri().to_string())
        })
        .collect()
}

fn decode_uri_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    let mut entry_count = 0;
    for line in lines {
        let uri = line.trim_end_matches('\r').trim().trim_end_matches('\0');
        if uri.is_empty() || uri.starts_with('#') {
            continue;
        }
        if !uri.starts_with("file:///") {
            return Err(format!("unsupported clipboard URI: {uri}"));
        }
        entry_count += 1;
        validate_entry_count(entry_count)?;
        let path = gio::File::for_uri(uri)
            .path()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| format!("unsupported clipboard URI: {uri}"))?;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err("the clipboard does not contain local file paths".to_owned());
    }
    Ok(paths)
}

fn decode_private(text: &str) -> Result<ClipboardImport, String> {
    let mut lines = text.lines();
    if lines.next() != Some("polarexp-v1") {
        return Err("the PolarExp clipboard version is unsupported".to_owned());
    }
    let generation = lines
        .next()
        .and_then(|line| line.strip_prefix("generation="))
        .ok_or_else(|| "the PolarExp clipboard generation is missing".to_owned())?
        .parse::<u64>()
        .map_err(|_| "the PolarExp clipboard generation is invalid".to_owned())?;
    let action = match lines.next().and_then(|line| line.strip_prefix("action=")) {
        Some("copy") => Action::Copy,
        Some("cut") => Action::Move,
        _ => return Err("the PolarExp clipboard action is invalid".to_owned()),
    };
    Ok(ClipboardImport {
        paths: decode_uri_lines(lines)?,
        action,
        generation: Some(generation),
    })
}

fn validate_entry_count(count: usize) -> Result<(), String> {
    if count == 0 {
        Err("the clipboard does not contain local file paths".to_owned())
    } else if count > MAX_ENTRIES {
        Err(format!(
            "the clipboard contains more than {MAX_ENTRIES} entries"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(action: Action) -> ClipboardPayload {
        ClipboardPayload {
            paths: vec![PathBuf::from("/tmp/one file"), PathBuf::from("/tmp/two")],
            action,
            generation: 42,
        }
    }

    #[test]
    fn one_offer_encodes_every_interoperable_clipboard_format() {
        let encoded = encode(&payload(Action::Copy)).unwrap();

        assert_eq!(
            encoded.mime_types().collect::<Vec<_>>(),
            [POLAREXP_MIME, GNOME_MIME, URI_LIST_MIME, KDE_CUT_MIME]
        );
        assert_eq!(encoded.data(KDE_CUT_MIME), Some(b"0".as_slice()));
        assert_eq!(
            std::str::from_utf8(encoded.data(GNOME_MIME).unwrap()).unwrap(),
            "copy\nfile:///tmp/one%20file\nfile:///tmp/two\n"
        );
    }

    #[test]
    fn decoding_prefers_action_with_paths_and_falls_back_to_copy() {
        let gnome = b"cut\nfile:///tmp/one\nfile:///tmp/two\n";
        assert_eq!(decode(GNOME_MIME, gnome).unwrap().action, Action::Move);

        let uri_list = b"file:///tmp/one\r\nfile:///tmp/two\r\n";
        let decoded = decode(URI_LIST_MIME, uri_list).unwrap();
        assert_eq!(decoded.action, Action::Copy);
        assert_eq!(decoded.generation, None);
    }

    #[test]
    fn malformed_remote_oversized_and_excessive_payloads_are_rejected() {
        assert!(decode(URI_LIST_MIME, b"file://remote/tmp/one\r\n").is_err());
        assert!(decode(URI_LIST_MIME, &vec![b'x'; MAX_BYTES + 1]).is_err());

        let excessive =
            std::iter::repeat_n("file:///tmp/one\r\n", MAX_ENTRIES + 1).collect::<String>();
        assert!(decode(URI_LIST_MIME, excessive.as_bytes()).is_err());
        assert_eq!(
            decode(GNOME_MIME, b"move\nfile:///tmp/one\n")
                .unwrap()
                .action,
            Action::Copy
        );
    }

    #[test]
    fn private_payload_round_trips_generation_and_rejects_contradictions() {
        let encoded = encode(&payload(Action::Move)).unwrap();
        let decoded = decode(POLAREXP_MIME, encoded.data(POLAREXP_MIME).unwrap()).unwrap();

        assert_eq!(decoded.paths, payload(Action::Move).paths);
        assert_eq!(decoded.action, Action::Move);
        assert_eq!(decoded.generation, Some(42));

        let contradictory =
            b"polarexp-v1\ngeneration=1\ngeneration=2\naction=copy\nfile:///tmp/one\n";
        assert!(decode(POLAREXP_MIME, contradictory).is_err());
    }

    #[test]
    fn one_generation_decodes_all_formats_and_keeps_consistent_move() {
        let encoded = encode(&payload(Action::Move)).unwrap();
        let owned = encoded.into_entries();
        let entries = owned
            .iter()
            .map(|(mime, data)| (*mime, data.as_slice()))
            .collect::<Vec<_>>();

        assert_eq!(
            decode_offer(&entries).unwrap(),
            ClipboardImport {
                paths: payload(Action::Move).paths,
                action: Action::Move,
                generation: Some(42),
            }
        );
    }

    #[test]
    fn contradictory_or_malformed_cut_markers_are_downgraded_to_copy() {
        let uri = b"file:///tmp/one\r\n";
        let gnome = b"cut\nfile:///tmp/one\n";
        let contradictory = [
            (URI_LIST_MIME, uri.as_slice()),
            (GNOME_MIME, gnome.as_slice()),
            (KDE_CUT_MIME, b"0".as_slice()),
        ];
        assert_eq!(decode_offer(&contradictory).unwrap().action, Action::Copy);

        let malformed = [
            (URI_LIST_MIME, uri.as_slice()),
            (KDE_CUT_MIME, b"maybe".as_slice()),
        ];
        assert_eq!(decode_offer(&malformed).unwrap().action, Action::Copy);
    }

    #[test]
    fn pcmanfm_wayland_copy_and_cut_markers_are_combined_safely() {
        let copy_uri = b"file:///tmp/copy-probe.txt\0\n";
        let copy_gnome = b"copy\nfile:///tmp/copy-probe.txt\0";
        let copy = [
            (URI_LIST_MIME, copy_uri.as_slice()),
            (GNOME_MIME, copy_gnome.as_slice()),
            (KDE_CUT_MIME, b"".as_slice()),
        ];
        assert_eq!(decode_offer(&copy).unwrap().action, Action::Copy);

        let cut_uri = b"file:///tmp/cut-probe.txt\0\n";
        let cut = [
            (URI_LIST_MIME, cut_uri.as_slice()),
            (GNOME_MIME, b"".as_slice()),
            (KDE_CUT_MIME, b"1\0".as_slice()),
        ];
        assert_eq!(decode_offer(&cut).unwrap().action, Action::Move);
    }

    #[test]
    fn contradictory_file_lists_are_rejected() {
        let entries = [
            (URI_LIST_MIME, b"file:///tmp/one\r\n".as_slice()),
            (GNOME_MIME, b"copy\nfile:///tmp/two\n".as_slice()),
        ];

        assert!(decode_offer(&entries).is_err());
    }
}
