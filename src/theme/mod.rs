use std::{collections::HashMap, env, fs, path::PathBuf};

use gio::prelude::*;
use iced::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeColors {
    pub accent: Color,
    pub selection_foreground: Option<Color>,
}

pub fn interface_settings() -> Option<gio::Settings> {
    let source = gio::SettingsSchemaSource::default()?;
    let schema = source.lookup("org.gnome.desktop.interface", true)?;
    Some(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}

pub fn load(settings: Option<&gio::Settings>) -> Option<ThemeColors> {
    theme_css_candidates(settings)
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .find_map(|css| parse_theme_css(&css))
}

fn theme_css_candidates(settings: Option<&gio::Settings>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = env::var_os("HOME").map(PathBuf::from);
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    if let Some(config_home) = config_home {
        paths.push(config_home.join("gtk-4.0/gtk.css"));
        paths.push(config_home.join("gtk-3.0/gtk.css"));
    }

    let theme_name = env::var("GTK_THEME")
        .ok()
        .and_then(|value| value.split(':').next().map(str::to_owned))
        .filter(|value| !value.is_empty())
        .or_else(|| settings.map(|settings| settings.string("gtk-theme").to_string()))
        .filter(|value| !value.is_empty());

    if let Some(theme_name) = theme_name {
        let mut data_roots = Vec::new();
        if let Some(data_home) = env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
            data_roots.push(data_home);
        } else if let Some(home) = &home {
            data_roots.push(home.join(".local/share"));
        }
        if let Some(home) = &home {
            data_roots.push(home.join(".themes"));
        }
        data_roots.extend([
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]);

        for root in data_roots {
            let theme_root = if root.file_name().is_some_and(|name| name == ".themes") {
                root.join(&theme_name)
            } else {
                root.join("themes").join(&theme_name)
            };
            paths.push(theme_root.join("gtk-4.0/gtk.css"));
            paths.push(theme_root.join("gtk-3.0/gtk.css"));
        }
    }

    paths
}

fn parse_theme_css(css: &str) -> Option<ThemeColors> {
    let definitions: HashMap<_, _> = css
        .lines()
        .filter_map(|line| {
            let definition = line.trim().strip_prefix("@define-color ")?;
            let (name, value) = definition.split_once(char::is_whitespace)?;
            Some((
                name.to_owned(),
                value.trim().trim_end_matches(';').to_owned(),
            ))
        })
        .collect();

    let accent = resolve_first(
        &definitions,
        &[
            "theme_selected_bg_color",
            "selected_bg_color",
            "accent_bg_color",
            "accent_color",
        ],
    )?;
    let selection_foreground = resolve_first(
        &definitions,
        &[
            "theme_selected_fg_color",
            "selected_fg_color",
            "accent_fg_color",
        ],
    );
    Some(ThemeColors {
        accent,
        selection_foreground,
    })
}

fn resolve_first(definitions: &HashMap<String, String>, names: &[&str]) -> Option<Color> {
    names
        .iter()
        .find_map(|name| resolve_color(definitions, name, 0))
}

fn resolve_color(definitions: &HashMap<String, String>, name: &str, depth: usize) -> Option<Color> {
    if depth > 8 {
        return None;
    }
    let value = definitions.get(name)?.trim();
    parse_css_color(value).or_else(|| {
        let reference = value.strip_prefix('@').unwrap_or(value);
        (reference != value || definitions.contains_key(reference))
            .then(|| resolve_color(definitions, reference, depth + 1))
            .flatten()
    })
}

fn parse_css_color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let expanded;
        let hex = match hex.len() {
            3 | 4 => {
                expanded = hex
                    .chars()
                    .flat_map(|digit| [digit, digit])
                    .collect::<String>();
                expanded.as_str()
            }
            6 | 8 => hex,
            _ => return None,
        };
        let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let alpha = if hex.len() == 8 {
            u8::from_str_radix(&hex[6..8], 16).ok()?
        } else {
            255
        };
        return Some(Color::from_rgba8(red, green, blue, alpha as f32 / 255.0));
    }

    match value.to_ascii_lowercase().as_str() {
        "white" => Some(Color::WHITE),
        "black" => Some(Color::BLACK),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
