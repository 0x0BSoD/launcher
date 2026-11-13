use iced::{
    Color, Theme,
    theme::{Mode, Palette},
};
use std::{collections::HashSet, fs, io, path::PathBuf};

#[derive(Debug, PartialEq)]
pub struct Config {
    pub round_edges: bool,
    pub dark: Palette,
    pub light: Palette,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            round_edges: true,
            dark: Theme::CatppuccinMocha.palette(),
            light: Theme::CatppuccinLatte.palette(),
        }
    }
}

// XDG base directory: the variable when absolute, otherwise $HOME/<fallback>.
pub fn xdg_dir(xdg: Option<PathBuf>, home: Option<PathBuf>, fallback: &str) -> Option<PathBuf> {
    xdg.filter(|path| path.is_absolute()).or_else(|| {
        home.filter(|path| path.is_absolute())
            .map(|path| path.join(fallback))
    })
}

fn config_path(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg_dir(xdg, home, ".config").map(|path| path.join("launcher/config"))
}

impl Config {
    pub fn load() -> Self {
        let path = config_path(
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
        );
        let Some(path) = path else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(contents) => Self::parse(&contents).unwrap_or_else(|error| {
                eprintln!("{}: {error}; using defaults", path.display());
                Self::default()
            }),
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    eprintln!("{}: {error}; using defaults", path.display());
                }
                Self::default()
            }
        }
    }

    pub fn radius(&self) -> f32 {
        if self.round_edges { 16.0 } else { 0.0 }
    }

    pub fn theme(&self, mode: Mode) -> Theme {
        let palette = if mode == Mode::Light {
            self.light
        } else {
            self.dark
        };
        Theme::custom("Launcher", palette)
    }

    // Flat key: value format, not full YAML; a color's leading # is part of its value.
    fn parse(contents: &str) -> Result<Self, String> {
        let mut config = Self::default();
        let mut seen = HashSet::new();
        for (line_number, line) in contents.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let invalid = || format!("invalid configuration at line {}", line_number + 1);
            let (key, value) = line.split_once(':').ok_or_else(invalid)?;
            let key = key.trim();
            let mut words = value.split_whitespace();
            let value = words.next().unwrap_or_default();
            if words.next().is_some_and(|word| !word.starts_with('#')) || !seen.insert(key) {
                return Err(invalid());
            }
            if key == "roundEdges" {
                config.round_edges = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err(invalid()),
                };
                continue;
            }
            let (appearance, property) = key.split_once('.').ok_or_else(invalid)?;
            let palette = match appearance {
                "dark" => &mut config.dark,
                "light" => &mut config.light,
                _ => return Err(invalid()),
            };
            let target = match property {
                "background" => &mut palette.background,
                "foreground" => &mut palette.text,
                "accent" => &mut palette.primary,
                _ => return Err(invalid()),
            };
            match parse_color(value) {
                Some(color) => *target = color,
                None => eprintln!(
                    "invalid color for {key} at line {}; using default",
                    line_number + 1
                ),
            }
        }
        Ok(config)
    }
}

fn parse_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::from_rgb8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colors_override_independently_and_preserve_comments() {
        let config = Config::parse("roundEdges: false # square\ndark.background: #010203 # background\nlight.foreground: #AbCdEf\ndark.accent: invalid").unwrap();
        assert!(!config.round_edges);
        assert_eq!(config.dark.background, Color::from_rgb8(1, 2, 3));
        assert_eq!(config.light.text, Color::from_rgb8(171, 205, 239));
        assert_eq!(config.dark.primary, Config::default().dark.primary);
        assert_eq!(config.dark.text, Config::default().dark.text);
        assert_eq!(config.theme(Mode::Light).palette(), config.light);
        assert_eq!(config.theme(Mode::Dark).palette(), config.dark);
        assert_eq!(config.theme(Mode::None).palette(), config.dark);
        for invalid in ["#fff", "#12345678", "#gggggg", "123456", "#é1234", ""] {
            assert!(parse_color(invalid).is_none());
        }
        assert!(Config::parse("dark.background: #000000\ndark.background: #ffffff").is_err());
    }

    #[test]
    fn example_matches_defaults() {
        assert_eq!(
            Config::parse(include_str!("../config/config")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn resolves_xdg_paths() {
        let home = Some(PathBuf::from("/home/test"));
        assert_eq!(
            config_path(Some("/custom".into()), home.clone()),
            Some("/custom/launcher/config".into())
        );
        for xdg in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(
                config_path(xdg, home.clone()),
                Some("/home/test/.config/launcher/config".into())
            );
        }
        assert_eq!(config_path(None, None), None);
    }

    #[test]
    fn parses_boolean_and_validates_input() {
        assert!(Config::parse("").unwrap().round_edges);
        assert!(
            Config::parse("# comment\nroundEdges: true")
                .unwrap()
                .round_edges
        );
        assert!(
            !Config::parse("roundEdges: false # square")
                .unwrap()
                .round_edges
        );
        for invalid in [
            "roundEdges: yes",
            "roundEdges: true\nroundEdges: false",
            "typo: true",
            "roundEdges=true",
        ] {
            assert!(Config::parse(invalid).is_err());
        }
    }
}
