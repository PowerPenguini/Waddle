use std::{env, fs, path::PathBuf, process::Command, sync::OnceLock};

use gio::prelude::SettingsExt;
use iced::advanced::graphics::text::{cosmic_text::fontdb, font_system};

const BUNDLED_FONTS: [&[u8]; 3] = [
    include_bytes!("../../data/fonts/AdwaitaSans-Regular.ttf"),
    include_bytes!("../../data/fonts/AdwaitaMono-Regular.ttf"),
    include_bytes!("../../data/fonts/AdwaitaMono-Bold.ttf"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Selection {
    pub(super) ui: iced::Font,
    pub(super) mono: iced::Font,
}

impl Selection {
    pub(super) fn ui_semibold(self) -> iced::Font {
        iced::Font {
            weight: iced::font::Weight::Semibold,
            ..self.ui
        }
    }

    pub(super) fn mono_semibold(self) -> iced::Font {
        iced::Font {
            weight: iced::font::Weight::Semibold,
            ..self.mono
        }
    }
}

pub(super) const fn bundled() -> Selection {
    Selection {
        ui: iced::Font::with_name("Waddle Sans"),
        mono: iced::Font::with_name("Waddle Mono"),
    }
}

pub(super) fn selected(system: bool) -> Selection {
    static SYSTEM: OnceLock<Selection> = OnceLock::new();
    let detected = *SYSTEM.get_or_init(load);
    if system { detected } else { bundled() }
}

fn load() -> Selection {
    let (ui, mono) = desktop_descriptions();
    let ui_fallback = fontconfig_family("sans-serif");
    let mono_fallback = fontconfig_family("monospace");
    let mut system = font_system().write().expect("font system lock poisoned");
    add_bundled_fonts(system.raw().db_mut());
    let (ui, mono) = select_families(
        system.raw().db(),
        ui.as_deref(),
        mono.as_deref(),
        ui_fallback.as_deref(),
        mono_fallback.as_deref(),
    );
    // Iced requires static family names. These two strings are interned once for
    // the process lifetime; switching sources does not allocate or reload fonts.
    Selection {
        ui: iced::Font::with_name(Box::leak(ui.into_boxed_str())),
        mono: iced::Font::with_name(Box::leak(mono.into_boxed_str())),
    }
}

fn add_bundled_fonts(db: &mut fontdb::Database) {
    let mut embedded = fontdb::Database::new();
    for bytes in BUNDLED_FONTS {
        embedded.load_font_data(bytes.to_vec());
    }
    // Private in-memory aliases prevent an installed font with the same family
    // name from shadowing the embedded version. The font files stay unmodified.
    for mut face in embedded.faces().cloned() {
        let name = if face.monospaced {
            "Waddle Mono"
        } else {
            "Waddle Sans"
        };
        for (family, _) in &mut face.families {
            *family = name.to_owned();
        }
        db.push_face_info(face);
    }
}

fn select_families(
    db: &fontdb::Database,
    ui: Option<&str>,
    mono: Option<&str>,
    ui_fallback: Option<&str>,
    mono_fallback: Option<&str>,
) -> (String, String) {
    (
        choose_family(db, ui, ui_fallback, "Waddle Sans", false)
            .expect("bundled UI font is loaded"),
        choose_family(db, mono, mono_fallback, "Waddle Mono", true)
            .expect("bundled mono font is loaded"),
    )
}

fn choose_family(
    db: &fontdb::Database,
    description: Option<&str>,
    fallback: Option<&str>,
    bundled: &str,
    monospace: bool,
) -> Option<String> {
    let available = db
        .faces()
        .filter(|face| !monospace || face.monospaced)
        .flat_map(|face| face.families.iter().map(|(name, _)| name.as_str()))
        .collect::<Vec<_>>();
    description
        .and_then(|description| matching_family(description, &available))
        .or_else(|| {
            fallback.and_then(|name| {
                available
                    .iter()
                    .copied()
                    .find(|family| family.eq_ignore_ascii_case(name))
            })
        })
        .or_else(|| available.iter().copied().find(|name| *name == bundled))
        .map(str::to_owned)
}

// Pango descriptions append style and size to a family. Matching installed names
// first preserves multi-word families and names containing digits.
fn matching_family<'a>(description: &str, available: &[&'a str]) -> Option<&'a str> {
    let description = description.trim();
    available
        .iter()
        .copied()
        .filter(|name| {
            description
                .get(..name.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
                && description.get(name.len()..).is_some_and(|suffix| {
                    suffix.is_empty() || suffix.starts_with(char::is_whitespace)
                })
        })
        .max_by_key(|name| name.len())
}

fn fontconfig_family(generic: &str) -> Option<String> {
    let output = Command::new("fc-match")
        .args(["--format=%{family[0]}", generic])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let family = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!family.is_empty()).then_some(family)
}

fn desktop_descriptions() -> (Option<String>, Option<String>) {
    let directories = config_directories();
    let kde = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .any(|desktop| desktop.eq_ignore_ascii_case("KDE"));
    if kde {
        let read = |key| {
            read_setting(&directories, "kdeglobals", "General", key).and_then(|value| {
                value
                    .split(',')
                    .next()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            })
        };
        return (read("font"), read("fixed"));
    }
    let settings = crate::theme::interface_settings();
    let read = |key| {
        settings
            .as_ref()
            .filter(|settings| {
                settings
                    .settings_schema()
                    .is_some_and(|schema| schema.has_key(key))
            })
            .map(|settings| settings.string(key).to_string())
            .filter(|value| !value.trim().is_empty())
    };
    let ui = read_setting(
        &directories,
        "gtk-4.0/settings.ini",
        "Settings",
        "gtk-font-name",
    )
    .or_else(|| {
        read_setting(
            &directories,
            "gtk-3.0/settings.ini",
            "Settings",
            "gtk-font-name",
        )
    })
    .or_else(|| read("font-name"));
    (ui, read("monospace-font-name"))
}

fn config_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(home) = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    {
        directories.push(home);
    }
    directories.extend(env::split_paths(
        &env::var_os("XDG_CONFIG_DIRS")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/etc/xdg".into()),
    ));
    directories
}

fn read_setting(directories: &[PathBuf], file: &str, section: &str, key: &str) -> Option<String> {
    directories.iter().find_map(|directory| {
        let contents = fs::read_to_string(directory.join(file)).ok()?;
        ini_value(&contents, section, key).map(str::to_owned)
    })
}

fn ini_value<'a>(contents: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut selected = false;
    let mut value = None;
    for line in contents.lines().map(str::trim) {
        if let Some(header) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            selected = header == section;
        } else if selected
            && let Some((name, setting)) = line.split_once('=')
            && name.trim() == key
        {
            let setting = setting.trim().trim_matches('"');
            value = (!setting.is_empty()).then_some(setting);
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_render_without_any_system_fonts_or_settings() {
        let mut db = fontdb::Database::new();
        add_bundled_fonts(&mut db);
        for descriptions in [
            (None, None),
            (Some("Missing Font 11"), Some("Missing Mono 10")),
        ] {
            let (ui, mono) = select_families(&db, descriptions.0, descriptions.1, None, None);
            assert_eq!(ui, "Waddle Sans");
            assert_eq!(mono, "Waddle Mono");
            for family in [&ui, &mono] {
                assert!(
                    db.query(&fontdb::Query {
                        families: &[fontdb::Family::Name(family)],
                        ..Default::default()
                    })
                    .is_some()
                );
            }
        }
    }

    #[test]
    fn available_desktop_families_take_precedence_over_bundled_fonts() {
        let mut db = fontdb::Database::new();
        add_bundled_fonts(&mut db);
        // Rename cloned face metadata to model an independently installed family.
        let mut face = db.faces().find(|face| !face.monospaced).unwrap().clone();
        face.families[0].0 = "Example Desktop Sans".to_owned();
        db.push_face_info(face);
        let (ui, mono) =
            select_families(&db, Some("Example Desktop Sans Bold 11"), None, None, None);
        assert_eq!(ui, "Example Desktop Sans");
        assert_eq!(mono, "Waddle Mono");
        let (ui, _) = select_families(
            &db,
            Some("Missing Font 11"),
            None,
            Some("Example Desktop Sans"),
            None,
        );
        assert_eq!(ui, "Example Desktop Sans");
        assert_eq!(bundled().ui.family, iced::font::Family::Name("Waddle Sans"));

        use iced::advanced::graphics::text::cosmic_text::{
            Attrs, Buffer, FontSystem, Metrics, Shaping,
        };
        let mut system = FontSystem::new_with_locale_and_db("en-US".to_owned(), db);
        let mut buffer = Buffer::new(&mut system, Metrics::new(16.0, 20.0));
        buffer.set_size(&mut system, Some(600.0), Some(60.0));
        for family in [
            "Waddle Sans",
            "Example Desktop Sans",
            "Waddle Mono",
            "Waddle Sans",
        ] {
            buffer.set_text(
                &mut system,
                "Home Documents 0123",
                &Attrs::new().family(fontdb::Family::Name(family)),
                Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(&mut system, false);
            let glyphs = buffer
                .layout_runs()
                .flat_map(|run| run.glyphs)
                .collect::<Vec<_>>();
            assert!(!glyphs.is_empty());
            for glyph in glyphs {
                assert_ne!(glyph.glyph_id, 0);
                assert!(
                    system
                        .db()
                        .face(glyph.font_id)
                        .unwrap()
                        .families
                        .iter()
                        .any(|(name, _)| name == family)
                );
            }
        }
    }

    #[test]
    fn parses_multiword_families_without_confusing_styles_or_digits() {
        let families = ["Noto Sans", "Noto Sans Mono", "DIN 1451"];
        assert_eq!(
            matching_family("Noto Sans Mono Bold 11", &families),
            Some("Noto Sans Mono")
        );
        assert_eq!(
            matching_family("DIN 1451 10.5", &families),
            Some("DIN 1451")
        );
        assert_eq!(matching_family("Noto Sansish 11", &families), None);
        assert_eq!(matching_family("Missing Font 11", &families), None);
    }

    #[test]
    fn reads_gtk_and_kde_settings_from_the_right_section() {
        assert_eq!(
            ini_value(
                "[Other]\ngtk-font-name=Wrong 12\n[Settings]\ngtk-font-name=\"Adwaita Sans 11\"",
                "Settings",
                "gtk-font-name"
            ),
            Some("Adwaita Sans 11")
        );
        assert_eq!(
            ini_value(
                "[General]\nfont=Noto Sans,10,-1,5,50,0\nfixed=Noto Sans Mono,10,-1,5,50,0",
                "General",
                "font"
            ),
            Some("Noto Sans,10,-1,5,50,0")
        );
        assert_eq!(
            ini_value("[Settings]\ngtk-font-name=", "Settings", "gtk-font-name"),
            None
        );
    }
}
