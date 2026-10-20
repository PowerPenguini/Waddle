use std::path::PathBuf;

pub(crate) fn local_path(uri: &str) -> Option<PathBuf> {
    if uri.contains('\0') {
        return None;
    }
    let (path, hostname) = gio::glib::filename_from_uri(uri).ok()?;
    if !path.is_absolute()
        || hostname.is_some_and(|hostname| !hostname.eq_ignore_ascii_case("localhost"))
    {
        return None;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_file_uris_preserve_escaped_and_non_utf8_path_bytes() {
        use std::os::unix::ffi::OsStrExt;

        for uri in [
            "file:///tmp/a%20file%FF",
            "file://localhost/tmp/a%20file%FF",
            "file://LOCALHOST/tmp/a%20file%FF",
            "file:/tmp/a%20file%FF",
        ] {
            assert_eq!(
                local_path(uri).unwrap().as_os_str().as_bytes(),
                b"/tmp/a file\xff",
                "{uri}"
            );
        }
    }

    #[test]
    fn remote_relative_and_malformed_file_uris_are_rejected() {
        for uri in [
            "file://remote/tmp/file",
            "file://localhost.remote/tmp/file",
            "file:relative",
            "https://localhost/tmp/file",
            "file:///tmp/%00file",
            "file:///tmp/a\0file",
            "file:///tmp/%ZZfile",
        ] {
            assert!(local_path(uri).is_none(), "{uri}");
        }
    }
}
