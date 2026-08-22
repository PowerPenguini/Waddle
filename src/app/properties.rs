use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use gio::prelude::AppInfoExt;

#[derive(Clone, Debug)]
pub(super) struct Info {
    pub(super) name: String,
    pub(super) detail: String,
}

pub(super) fn read(path: &Path) -> Result<Info, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    let content_type = content_type(path, metadata.is_dir());
    let mime = gio::content_type_get_mime_type(&content_type)
        .map_or_else(|| content_type.to_string(), |mime| mime.to_string());
    let kind = if metadata.file_type().is_symlink() {
        "Symbolic link".to_owned()
    } else {
        gio::content_type_get_description(&content_type).to_string()
    };
    let permissions = permissions(metadata.mode());
    let default = gio::AppInfo::default_for_type(&content_type, false)
        .map(|app| format!("{} ({})", app.name(), app_id(&app)))
        .unwrap_or_else(|| "None".to_owned());
    let mut applications = gio::AppInfo::all_for_type(&content_type)
        .into_iter()
        .map(|app| format!("{}  {}", app_id(&app), app.name()))
        .collect::<Vec<_>>();
    applications.sort();
    applications.dedup();
    let applications = if applications.is_empty() {
        "  None".to_owned()
    } else {
        applications
            .into_iter()
            .map(|application| format!("  {application}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let location = path.parent().unwrap_or(path);
    Ok(Info {
        name,
        detail: format!(
            "Type: {kind}\nMIME type: {mime}\nLocation: {}\nSize: {}\nModified: {}\nAccessed: {}\nChanged: {}\nPermissions: {permissions} ({:04o})\nOwner: {}:{}\nDefault application: {default}\n\nOpen With applications:\n{applications}\n\nUse :chmod MODE, :open-with APP_ID, or :default-app APP_ID.",
            location.display(),
            gio::glib::format_size(metadata.len()),
            date(metadata.mtime()),
            date(metadata.atime()),
            date(metadata.ctime()),
            metadata.mode() & 0o7777,
            metadata.uid(),
            metadata.gid(),
        ),
    })
}

pub(super) fn chmod(paths: Vec<PathBuf>, mode: &str) -> Result<String, String> {
    let mode = mode.trim();
    if mode.len() < 3 || mode.len() > 4 || !mode.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
        return Err(
            "permission mode must be three or four octal digits, for example 640".to_owned(),
        );
    }
    let mode = u32::from_str_radix(mode, 8).map_err(|error| error.to_string())?;
    let mut changed = 0;
    let mut failures = Vec::new();
    for path in paths {
        match fs::set_permissions(&path, fs::Permissions::from_mode(mode)) {
            Ok(()) => changed += 1,
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    if failures.is_empty() {
        Ok(format!(
            "Changed permissions on {changed} items to {mode:04o}"
        ))
    } else {
        Err(format!(
            "Changed permissions on {changed} items; {} failed\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

pub(super) fn open_with(
    path: PathBuf,
    requested: &str,
    make_default: bool,
) -> Result<String, String> {
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    let content_type = content_type(&path, metadata.is_dir());
    let requested = requested.trim();
    if requested.is_empty() {
        return Err("application ID is required; open Properties to list candidates".to_owned());
    }
    let application = gio::AppInfo::all_for_type(&content_type)
        .into_iter()
        .find(|application| {
            app_id(application).eq_ignore_ascii_case(requested)
                || application.name().eq_ignore_ascii_case(requested)
        })
        .ok_or_else(|| format!("application not found for this file type: {requested}"))?;
    if make_default {
        application
            .set_as_default_for_type(&content_type)
            .map_err(|error| format!("Could not set the default application: {error}"))?;
        Ok(format!(
            "{} is now the default for {}",
            application.name(),
            gio::content_type_get_mime_type(&content_type).unwrap_or(content_type)
        ))
    } else {
        application
            .launch(
                &[gio::File::for_path(&path)],
                None::<&gio::AppLaunchContext>,
            )
            .map_err(|error| format!("Could not launch {}: {error}", application.name()))?;
        Ok(format!("Opened with {}", application.name()))
    }
}

fn app_id(application: &gio::AppInfo) -> String {
    application
        .id()
        .map_or_else(|| application.name().to_string(), |id| id.to_string())
}

fn content_type(path: &Path, directory: bool) -> gio::glib::GString {
    if directory {
        return "inode/directory".into();
    }
    gio::content_type_guess(Some(path), None).0
}

fn date(seconds: i64) -> String {
    gio::glib::DateTime::from_unix_local(seconds)
        .and_then(|date| date.format("%Y-%m-%d %H:%M:%S %Z"))
        .map_or_else(|_| "Unavailable".to_owned(), |date| date.to_string())
}

fn permissions(mode: u32) -> String {
    let mut value = String::with_capacity(9);
    for (read, write, execute) in [
        (0o400, 0o200, 0o100),
        (0o040, 0o020, 0o010),
        (0o004, 0o002, 0o001),
    ] {
        value.push(if mode & read != 0 { 'r' } else { '-' });
        value.push(if mode & write != 0 { 'w' } else { '-' });
        value.push(if mode & execute != 0 { 'x' } else { '-' });
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_include_required_metadata_and_permission_edits_stay_user_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("notes.txt");
        fs::write(&path, "hello").unwrap();
        let info = read(&path).unwrap();
        for label in [
            "Type:",
            "MIME type:",
            "Location:",
            "Size:",
            "Modified:",
            "Accessed:",
            "Permissions:",
            "Default application:",
        ] {
            assert!(info.detail.contains(label), "missing {label}");
        }

        assert!(chmod(vec![path.clone()], "640").is_ok());
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert!(chmod(Vec::new(), "u+rwx").is_err());
    }
}
