use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Template {
    pub(super) path: PathBuf,
    pub(super) label: String,
    pub(super) suggested_name: String,
}

pub(super) fn discover() -> Vec<Template> {
    gio::glib::user_special_dir(gio::glib::UserDirectory::Templates)
        .filter(|path| path.is_dir())
        .map_or_else(Vec::new, |path| discover_at(&path))
}

fn discover_at(directory: &std::path::Path) -> Vec<Template> {
    let mut templates = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            let suggested_name = entry.file_name().to_string_lossy().into_owned();
            Template {
                path: entry.path(),
                label: suggested_name.clone(),
                suggested_name,
            }
        })
        .collect::<Vec<_>>();
    templates.sort_by_key(|template| template.label.to_lowercase());
    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_lists_existing_files_without_creating_the_directory() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        assert!(discover_at(&missing).is_empty());
        assert!(!missing.exists());

        let directory = temp.path().join("Templates");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("Letter.md"), "letter").unwrap();
        fs::create_dir(directory.join("Folder")).unwrap();
        let templates = discover_at(&directory);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].suggested_name, "Letter.md");
    }
}
