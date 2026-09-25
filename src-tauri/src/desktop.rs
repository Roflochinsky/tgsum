//! Linux desktop integration: tiling Wayland compositors and Omarchy themes.
//!
//! Both helpers take the environment as a lookup function so they can be
//! tested without touching the real process environment.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Compositors that tile windows without title bars (Omarchy runs Hyprland).
const TILING_DESKTOPS: [&str; 5] = ["hyprland", "sway", "niri", "river", "dwl"];

/// Sockets only these compositors export.
const TILING_SOCKETS: [&str; 3] = ["HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK", "NIRI_SOCKET"];

/// Whether GTK will talk Wayland to a tiling compositor.
pub fn tiling_wayland(var: impl Fn(&str) -> Option<String>) -> bool {
    let wayland = var("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
        && !var("GDK_BACKEND").is_some_and(|b| b.trim_start().starts_with("x11"));
    if !wayland {
        return false;
    }
    let desktop = var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    desktop.split(':').any(|d| TILING_DESKTOPS.contains(&d))
        || TILING_SOCKETS.iter().any(|name| var(name).is_some())
}

/// Whether the main window gets the system title bar. Tiling compositors show
/// windows without one, so there the app's own header is the title bar (and
/// the GTK fallback bar tao 0.35 draws on Wayland has unreliable buttons).
/// `TGSUM_DECORATIONS=0|1` overrides the detection.
pub fn native_decorations(var: impl Fn(&str) -> Option<String>) -> bool {
    match var("TGSUM_DECORATIONS").as_deref().map(str::trim) {
        Some("0" | "false" | "no" | "off") => false,
        Some("1" | "true" | "yes" | "on") => true,
        _ => !(cfg!(target_os = "linux") && tiling_wayland(var)),
    }
}

/// Colors of the desktop theme the UI adopts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTheme {
    /// Where the colors come from: `"omarchy"`.
    pub source: &'static str,
    pub name: Option<String>,
    /// `"dark"` or `"light"`.
    pub mode: &'static str,
    /// `#rrggbb` colors by key: always `accent`, `background`, `foreground`;
    /// then whatever the theme defines (`color1`, `red`, `lighter_background`…).
    pub colors: BTreeMap<String, String>,
}

/// The active Omarchy theme when `TGSUM_THEME=omarchy` asks the app to wear
/// it instead of its own painting-based design. Omarchy 4 keeps it in
/// `~/.local/state/omarchy/current/theme`, Omarchy 3 in
/// `~/.config/omarchy/current/theme`.
pub fn omarchy_theme(var: impl Fn(&str) -> Option<String>) -> Option<DesktopTheme> {
    if var("TGSUM_THEME").as_deref().map(str::trim) != Some("omarchy") {
        return None;
    }
    let dir = |xdg: &str, fallback: &str| -> Option<PathBuf> {
        match var(xdg).filter(|d| !d.is_empty()) {
            Some(d) => Some(PathBuf::from(d)),
            None => var("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| Path::new(&h).join(fallback)),
        }
    };
    [
        dir("XDG_STATE_HOME", ".local/state"),
        dir("XDG_CONFIG_HOME", ".config"),
    ]
    .into_iter()
    .flatten()
    .find_map(|base| load_omarchy_theme(&base.join("omarchy/current")))
}

/// Reads `<current>/theme/colors.toml`; the mode comes from its `mode` key,
/// a `light.mode` marker file, or the background's brightness.
pub fn load_omarchy_theme(current: &Path) -> Option<DesktopTheme> {
    let theme_dir = current.join("theme");
    let (mode, colors) = parse_colors(&fs::read_to_string(theme_dir.join("colors.toml")).ok()?);
    if !["accent", "background", "foreground"]
        .iter()
        .all(|k| colors.contains_key(*k))
    {
        return None;
    }
    let light = match mode.as_deref() {
        Some("light") => true,
        Some("dark") => false,
        _ => theme_dir.join("light.mode").exists() || luminance(&colors["background"]) > 0.5,
    };
    let name = fs::read_to_string(current.join("theme.name"))
        .ok()
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .or_else(|| {
            let target = fs::canonicalize(&theme_dir).ok()?;
            Some(target.file_name()?.to_string_lossy().into_owned())
        });
    Some(DesktopTheme {
        source: "omarchy",
        name,
        mode: if light { "light" } else { "dark" },
        colors,
    })
}

/// `key = "value"` lines of a flat TOML file: the `mode` value and every
/// value that is a hex color (normalized to lowercase `#rrggbb`).
fn parse_colors(text: &str) -> (Option<String>, BTreeMap<String, String>) {
    let mut mode = None;
    let mut colors = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        let value = unquote(value.trim());
        if key == "mode" {
            mode = Some(value.to_ascii_lowercase());
        } else if let Some(hex) = hex_color(value) {
            colors.insert(key.to_owned(), hex);
        }
    }
    (mode, colors)
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(rest) = value.strip_prefix(quote) {
            return rest.split(quote).next().unwrap_or_default();
        }
    }
    value.split(" #").next().unwrap_or_default().trim()
}

fn hex_color(value: &str) -> Option<String> {
    let hex = value
        .strip_prefix('#')
        .or_else(|| value.strip_prefix("0x"))?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rgb: String = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => hex[..6].to_owned(), // alpha is dropped
        _ => return None,
    };
    Some(format!("#{}", rgb.to_ascii_lowercase()))
}

/// Relative luminance of a `#rrggbb` color (0 = black, 1 = white).
fn luminance(hex: &str) -> f64 {
    let channel = |i: usize| {
        let c = f64::from(u8::from_str_radix(&hex[1 + 2 * i..3 + 2 * i], 16).unwrap_or(0)) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn detects_tiling_wayland_compositors() {
        assert!(tiling_wayland(env(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("XDG_CURRENT_DESKTOP", "Hyprland")
        ])));
        assert!(tiling_wayland(env(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("XDG_CURRENT_DESKTOP", "sway:wlroots")
        ])));
        assert!(tiling_wayland(env(&[
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc")
        ])));
        // Not tiling, not Wayland, or GTK forced onto XWayland.
        assert!(!tiling_wayland(env(&[
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("XDG_CURRENT_DESKTOP", "GNOME")
        ])));
        assert!(!tiling_wayland(env(&[("XDG_CURRENT_DESKTOP", "Hyprland")])));
        assert!(!tiling_wayland(env(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("XDG_CURRENT_DESKTOP", "Hyprland"),
            ("GDK_BACKEND", "x11"),
        ])));
    }

    #[test]
    fn decorations_can_be_forced() {
        let hypr = [
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("XDG_CURRENT_DESKTOP", "Hyprland"),
        ];
        assert_eq!(native_decorations(env(&hypr)), !cfg!(target_os = "linux"));
        assert!(native_decorations(env(&[
            hypr[0],
            hypr[1],
            ("TGSUM_DECORATIONS", "1")
        ])));
        assert!(!native_decorations(env(&[("TGSUM_DECORATIONS", "0")])));
        assert!(native_decorations(env(&[])));
    }

    #[test]
    fn parses_both_omarchy_color_formats() {
        let (mode, colors) = parse_colors(
            "# Omarchy 3\naccent = \"#7AA2F7\"\ncursor = \"#c0caf5\"\nforeground = \"#a9b1d6\"\nbackground = '#1a1b26'\ncolor1 = \"#f7768e\"\n",
        );
        assert_eq!(mode, None);
        assert_eq!(colors["accent"], "#7aa2f7");
        assert_eq!(colors["background"], "#1a1b26");
        assert_eq!(colors["color1"], "#f7768e");

        let (mode, colors) = parse_colors(
            "mode = \"light\"\n\naccent = \"#1e66f5\" # blue\nbackground = \"#eff1f5\"\nlighter_background=\"#fff\"\nforeground = \"#4c4f69cc\"\nname = \"Latte\"\n",
        );
        assert_eq!(mode.as_deref(), Some("light"));
        assert_eq!(colors["lighter_background"], "#ffffff");
        assert_eq!(colors["foreground"], "#4c4f69");
        assert!(!colors.contains_key("name"));
    }

    fn write_theme(dir: &Path, colors: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("colors.toml"), colors).unwrap();
    }

    #[test]
    fn loads_the_current_theme_from_state_or_config_dir() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path().to_str().unwrap();
        let on = ("TGSUM_THEME", "omarchy");
        assert_eq!(omarchy_theme(env(&[("HOME", h), on])), None);

        // Omarchy 3: ~/.config/omarchy/current/theme, light themes ship `light.mode`.
        let v3 = home.path().join(".config/omarchy/current/theme");
        write_theme(
            &v3,
            "accent = \"#56949f\"\nbackground = \"#faf4ed\"\nforeground = \"#575279\"\n",
        );
        fs::write(v3.join("light.mode"), "").unwrap();
        let theme = omarchy_theme(env(&[("HOME", h), on])).unwrap();
        assert_eq!(theme.mode, "light");
        assert_eq!(theme.colors["accent"], "#56949f");

        // Omarchy 4 state dir wins; mode from the file, name from theme.name.
        let state = home.path().join(".local/state/omarchy/current");
        write_theme(
            &state.join("theme"),
            "mode = \"dark\"\naccent = \"#7aa2f7\"\nbackground = \"#1a1b26\"\nforeground = \"#a9b1d6\"\n",
        );
        fs::write(state.join("theme.name"), "tokyo-night\n").unwrap();
        let theme = omarchy_theme(env(&[("HOME", h), on])).unwrap();
        assert_eq!(
            (theme.mode, theme.name.as_deref()),
            ("dark", Some("tokyo-night"))
        );

        // The app's own design unless the theme is asked for.
        assert_eq!(omarchy_theme(env(&[("HOME", h)])), None);
        assert_eq!(
            omarchy_theme(env(&[("HOME", h), ("TGSUM_THEME", "builtin")])),
            None
        );
        let xdg = home.path().join("elsewhere");
        assert_eq!(
            omarchy_theme(env(&[
                ("HOME", h),
                ("XDG_STATE_HOME", xdg.to_str().unwrap()),
                on
            ]))
            .unwrap()
            .mode,
            "light"
        );
    }

    #[cfg(unix)]
    #[test]
    fn theme_name_falls_back_to_the_symlink_target() {
        let home = tempfile::tempdir().unwrap();
        let themes = home.path().join("themes/rose-pine");
        write_theme(
            &themes,
            "accent = \"#ebbcba\"\nbackground = \"#191724\"\nforeground = \"#e0def4\"\n",
        );
        let current = home.path().join("current");
        fs::create_dir_all(&current).unwrap();
        std::os::unix::fs::symlink(&themes, current.join("theme")).unwrap();
        let theme = load_omarchy_theme(&current).unwrap();
        assert_eq!(
            (theme.mode, theme.name.as_deref()),
            ("dark", Some("rose-pine"))
        );
    }

    #[test]
    fn incomplete_theme_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("theme")).unwrap();
        fs::write(dir.path().join("theme/colors.toml"), "accent = \"#fff\"\n").unwrap();
        assert_eq!(load_omarchy_theme(dir.path()), None);
    }

    #[test]
    fn brightness_decides_mode_without_hints() {
        assert!(luminance("#ffffff") > 0.99 && luminance("#000000") < 0.01);
        assert!(luminance("#1a1b26") < 0.5 && luminance("#eff1f5") > 0.5);
    }
}
