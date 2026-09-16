use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct AccessControl {
    access: Option<Vec<u8>>,
    default: Option<Vec<u8>>,
}

impl AccessControl {
    pub(super) fn read(path: &Path) -> Result<Option<Self>, Error> {
        let attributes = match crate::fs::read_xattrs(path) {
            Ok(attributes) => attributes,
            Err(error)
                if cfg!(not(target_os = "linux"))
                    && error.kind() == std::io::ErrorKind::Unsupported =>
            {
                return Ok(None);
            }
            Err(error) => return Err(Error::io("could not record creation access control", error)),
        };
        let mut saved = Self {
            access: None,
            default: None,
        };
        for (name, value) in attributes {
            match name.to_bytes() {
                b"system.posix_acl_access" => saved.access = Some(value),
                b"system.posix_acl_default" => saved.default = Some(value),
                _ => {}
            }
        }
        Ok(Some(saved))
    }

    #[cfg(target_os = "linux")]
    pub(super) fn restore(&self, path: &Path) -> Result<(), Error> {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| Error::message("creation path contains NUL"))?;
        for (name, value) in [
            (c"system.posix_acl_access", &self.access),
            (c"system.posix_acl_default", &self.default),
        ] {
            if let Some(value) = value {
                // SAFETY: path, name and value remain valid throughout this call.
                if unsafe {
                    libc::lsetxattr(
                        path.as_ptr(),
                        name.as_ptr(),
                        value.as_ptr().cast(),
                        value.len(),
                        0,
                    )
                } != 0
                {
                    return Err(Error::io(
                        "could not restore creation access control",
                        std::io::Error::last_os_error(),
                    ));
                }
            } else {
                // SAFETY: both arguments are live NUL-terminated strings.
                if unsafe { libc::lremovexattr(path.as_ptr(), name.as_ptr()) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if !matches!(error.raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)) {
                        return Err(Error::io(
                            "could not remove inherited creation access control",
                            error,
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn restore(&self, _: &Path) -> Result<(), Error> {
        Err(Error::message(
            "restoring creation access control is unsupported",
        ))
    }
}
