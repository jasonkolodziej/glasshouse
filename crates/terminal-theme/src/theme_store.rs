//! Built-in templates (read-only, embedded) and your own themes (YAML files in a folder), plus `appearance.yaml`.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::{
    AppearanceConfig, Palette, ResolvedAppearance, TerminalTheme, ThemeColor, ThemeError,
    ThemeImporter,
};

const THEME_HEADER: &str =
    "# Glasshouse terminal theme. See the README's Themes section for every key.\n";
const CONFIG_HEADER: &str =
    "# Glasshouse appearance settings. Settings › Appearance writes this file.\n";

/// The bundled templates as `(id, YAML)`.
const BUILT_IN_SOURCES: [(&str, &str); 15] = [
    (
        "catppuccin-latte",
        include_str!("../themes/catppuccin-latte.yaml"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../themes/catppuccin-mocha.yaml"),
    ),
    ("dracula", include_str!("../themes/dracula.yaml")),
    (
        "glasshouse-dark",
        include_str!("../themes/glasshouse-dark.yaml"),
    ),
    (
        "glasshouse-light",
        include_str!("../themes/glasshouse-light.yaml"),
    ),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.yaml")),
    (
        "gruvbox-light",
        include_str!("../themes/gruvbox-light.yaml"),
    ),
    (
        "high-contrast-dark",
        include_str!("../themes/high-contrast-dark.yaml"),
    ),
    (
        "high-contrast-light",
        include_str!("../themes/high-contrast-light.yaml"),
    ),
    ("nord", include_str!("../themes/nord.yaml")),
    ("one-dark", include_str!("../themes/one-dark.yaml")),
    (
        "solarized-dark",
        include_str!("../themes/solarized-dark.yaml"),
    ),
    (
        "solarized-light",
        include_str!("../themes/solarized-light.yaml"),
    ),
    (
        "terminal-basic",
        include_str!("../themes/terminal-basic.yaml"),
    ),
    ("tokyo-night", include_str!("../themes/tokyo-night.yaml")),
];

static BUILT_IN_THEMES: LazyLock<Vec<TerminalTheme>> = LazyLock::new(|| {
    let mut themes: Vec<TerminalTheme> = BUILT_IN_SOURCES
        .iter()
        .filter_map(|(id, text)| ThemeStore::theme_from_yaml(text, id).ok())
        .collect();
    themes.sort_by(|a, b| standard_compare(&a.name, &b.name));
    themes
});

/// A user theme file that couldn't be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFileProblem {
    /// The file name, such as `broken.yaml`.
    pub file: String,
    /// Why, for people.
    pub problem: String,
}

/// Built-in templates (read-only) and your themes (`<user_directory>/<id>.yaml`), plus `appearance.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeStore {
    /// The folder holding your themes.
    pub user_directory: PathBuf,
    /// Where `appearance.yaml` lives.
    pub config_path: PathBuf,
}

impl ThemeStore {
    /// A store with explicit locations.
    pub fn new(user_directory: impl Into<PathBuf>, config_path: impl Into<PathBuf>) -> Self {
        Self {
            user_directory: user_directory.into(),
            config_path: config_path.into(),
        }
    }

    /// `<support>/Glasshouse/Themes` and `<support>/Glasshouse/appearance.yaml`.
    pub fn in_support_directory(support: &Path) -> Self {
        let root = support.join("Glasshouse");
        Self::new(root.join("Themes"), root.join("appearance.yaml"))
    }

    /// The store in the platform's settings folder (see [`default_support_directory`]), or `None` when the home
    /// folder is unknown.
    pub fn standard() -> Option<Self> {
        default_support_directory().map(|support| Self::in_support_directory(&support))
    }

    // MARK: Themes

    /// The bundled templates, sorted by name.
    pub fn built_in_themes() -> &'static [TerminalTheme] {
        &BUILT_IN_THEMES
    }

    /// Whether `id` names a built-in template.
    pub fn is_built_in(id: &str) -> bool {
        Self::built_in_themes().iter().any(|theme| theme.id == id)
    }

    /// Your themes, sorted by name. Files that don't parse are left out; [`ThemeStore::user_theme_problems`] lists
    /// them.
    pub fn user_themes(&self) -> Vec<TerminalTheme> {
        let mut themes: Vec<TerminalTheme> = self
            .user_theme_files()
            .iter()
            .filter_map(|path| self.load(path).ok())
            .collect();
        themes.sort_by(|a, b| standard_compare(&a.name, &b.name));
        themes
    }

    /// File name and reason for every theme file that couldn't be read, in file-name order.
    pub fn user_theme_problems(&self) -> Vec<ThemeFileProblem> {
        self.user_theme_files()
            .iter()
            .filter_map(|path| {
                self.load(path).err().map(|error| ThemeFileProblem {
                    file: file_name(path),
                    problem: error.to_string(),
                })
            })
            .collect()
    }

    /// Built-in templates first, then your themes.
    pub fn all_themes(&self) -> Vec<TerminalTheme> {
        let mut themes = Self::built_in_themes().to_vec();
        themes.extend(self.user_themes());
        themes
    }

    /// The built-in or user theme with this id.
    pub fn theme(&self, id: &str) -> Option<TerminalTheme> {
        if let Some(theme) = Self::built_in_themes().iter().find(|theme| theme.id == id) {
            return Some(theme.clone());
        }
        if !Self::is_valid_id(id) {
            return None;
        }
        self.load(&self.file_path(id)).ok()
    }

    /// Validates and writes the theme. A theme without an id, or with a built-in or invalid one, gets a new id from
    /// its name. Returns the theme as saved.
    ///
    /// # Errors
    /// A validation error, or [`ThemeError::Io`] when the file can't be written.
    pub fn save(&self, theme: &TerminalTheme) -> Result<TerminalTheme, ThemeError> {
        let mut theme = theme.validated()?;
        if theme.id.is_empty() || Self::is_built_in(&theme.id) || !Self::is_valid_id(&theme.id) {
            theme.id = self.unique_id(&theme.name);
        }
        write_atomically(&self.file_path(&theme.id), Self::yaml(&theme)?.as_bytes())?;
        Ok(theme)
    }

    /// Saves a copy of `theme` under a new name and id, as a theme of yours.
    ///
    /// # Errors
    /// As [`ThemeStore::save`].
    pub fn duplicate(
        &self,
        theme: &TerminalTheme,
        name: &str,
    ) -> Result<TerminalTheme, ThemeError> {
        self.save(&theme.copy_named(name))
    }

    /// Deletes one of your themes.
    ///
    /// # Errors
    /// [`ThemeError::ReadOnly`] for a built-in, [`ThemeError::NotFound`] for an invalid or unknown id, or
    /// [`ThemeError::Io`].
    pub fn delete(&self, id: &str) -> Result<(), ThemeError> {
        if Self::is_built_in(id) {
            return Err(ThemeError::ReadOnly(id.to_owned()));
        }
        let path = self.file_path(id);
        if !Self::is_valid_id(id) || !path.exists() {
            return Err(ThemeError::NotFound(id.to_owned()));
        }
        fs::remove_file(&path).map_err(|error| io_error(&path, &error))
    }

    /// Reads a Glasshouse YAML theme (`.yaml`, `.yml`), an iTerm2 `.itermcolors` file or (any other extension) a
    /// Ghostty theme, and saves it as yours. A name already in use gets a number added.
    ///
    /// # Errors
    /// [`ThemeError::Unreadable`] when the file can't be read, the format's own errors, or those of
    /// [`ThemeStore::save`].
    pub fn import_theme(&self, path: &Path) -> Result<TerminalTheme, ThemeError> {
        let data = fs::read(path).map_err(|_| ThemeError::Unreadable(file_name(path)))?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let mut theme = match extension.as_str() {
            "itermcolors" => ThemeImporter::iterm(&data, &name)?,
            "yaml" | "yml" => Self::theme_from_yaml(&String::from_utf8_lossy(&data), "")?,
            _ => ThemeImporter::ghostty(&String::from_utf8_lossy(&data), &name)?,
        };
        theme.id = String::new();
        let name_taken = |name: &str| {
            Self::built_in_themes().iter().any(|t| t.name == name)
                || self.user_themes().iter().any(|t| t.name == name)
        };
        if name_taken(&theme.name) {
            theme.name = self.unique_name(&theme.name);
        }
        self.save(&theme)
    }

    // MARK: appearance.yaml

    /// The saved settings, or the defaults when there are none or they don't parse or validate.
    pub fn load_config(&self) -> AppearanceConfig {
        fs::read_to_string(&self.config_path)
            .ok()
            .and_then(|text| AppearanceConfig::from_yaml(&text).ok())
            .and_then(|config| config.validated().ok())
            .unwrap_or_default()
    }

    /// Validates and writes `appearance.yaml`.
    ///
    /// # Errors
    /// A validation error, or [`ThemeError::Io`].
    pub fn save_config(&self, config: &AppearanceConfig) -> Result<(), ThemeError> {
        let text = format!("{CONFIG_HEADER}{}", config.validated()?.to_yaml());
        write_atomically(&self.config_path, text.as_bytes())
    }

    /// The theme for the current system appearance, with every setting resolved. `config` defaults to
    /// [`ThemeStore::load_config`]. A missing theme falls back to Glasshouse Dark or Light.
    pub fn resolved(&self, dark: bool, config: Option<&AppearanceConfig>) -> ResolvedAppearance {
        let config = config.cloned().unwrap_or_else(|| self.load_config());
        let fallback = if dark {
            "glasshouse-dark"
        } else {
            "glasshouse-light"
        };
        let theme = self
            .theme(config.theme.theme_id(dark))
            .or_else(|| self.theme(fallback))
            .unwrap_or_else(Self::emergency_theme);
        ResolvedAppearance::new(theme, &config)
    }

    // MARK: YAML

    /// The file text for a theme: a header comment, then the validated theme.
    ///
    /// # Errors
    /// A validation error.
    pub fn yaml(theme: &TerminalTheme) -> Result<String, ThemeError> {
        Ok(format!(
            "{THEME_HEADER}{}",
            crate::yaml::encode_theme(&theme.validated()?)
        ))
    }

    /// Parses and validates a theme, giving it `id`.
    ///
    /// # Errors
    /// [`ThemeError::MissingKey`] with the dotted path of a missing key, the error a value raised (such as
    /// [`ThemeError::InvalidColor`]), a validation error, or [`ThemeError::Unreadable`] naming `id` ("The file" when
    /// empty) for anything else.
    pub fn theme_from_yaml(text: &str, id: &str) -> Result<TerminalTheme, ThemeError> {
        let mut theme =
            crate::yaml::decode_theme(text).map_err(|error| error.into_theme_error(id))?;
        theme.id = id.to_owned();
        theme.validated()
    }

    // MARK: Files

    /// 1–64 lowercase ASCII letters, digits and dashes, not starting with a dash.
    pub fn is_valid_id(id: &str) -> bool {
        !id.is_empty()
            && id.chars().count() <= 64
            && !id.starts_with('-')
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }

    /// `"My Theme!"` → `my-theme`, then `my-theme-2`, … until no theme has it.
    pub fn unique_id(&self, name: &str) -> String {
        let mapped: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let slug = mapped
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base: String = (if slug.is_empty() { "theme" } else { &slug })
            .chars()
            .take(56)
            .collect();
        let taken: HashSet<String> = self
            .all_themes()
            .into_iter()
            .map(|theme| theme.id)
            .collect();
        if !taken.contains(&base) {
            return base;
        }
        (2u64..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !taken.contains(id))
            .unwrap_or(base)
    }

    /// `name 2`, `name 3`, … whichever no theme has yet.
    pub fn unique_name(&self, name: &str) -> String {
        let taken: HashSet<String> = self
            .all_themes()
            .into_iter()
            .map(|theme| theme.name)
            .collect();
        (2u64..)
            .map(|n| format!("{name} {n}"))
            .find(|candidate| !taken.contains(candidate))
            .unwrap_or_else(|| name.to_owned())
    }

    /// Used only if the built-in themes are missing.
    pub fn emergency_theme() -> TerminalTheme {
        let mut theme = TerminalTheme::new(
            "Glasshouse Dark",
            Palette::new(
                ThemeColor::from_rgb8(0x16, 0x18, 0x1d),
                ThemeColor::from_rgb8(0xe6, 0xe6, 0xe3),
                TerminalTheme::GLASSHOUSE_NORMAL,
                TerminalTheme::GLASSHOUSE_BRIGHT,
            ),
        );
        theme.id = "glasshouse-dark".into();
        theme
    }

    fn file_path(&self, id: &str) -> PathBuf {
        self.user_directory.join(format!("{id}.yaml"))
    }

    fn user_theme_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.user_directory) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".yaml"))
            .collect();
        names.sort();
        names
            .into_iter()
            .map(|name| self.user_directory.join(name))
            .collect()
    }

    fn load(&self, path: &Path) -> Result<TerminalTheme, ThemeError> {
        let file = file_name(path);
        let id = file.strip_suffix(".yaml").unwrap_or(&file);
        if !Self::is_valid_id(id) || Self::is_built_in(id) {
            return Err(ThemeError::Unreadable(file));
        }
        let text = fs::read_to_string(path).map_err(|_| ThemeError::Unreadable(file.clone()))?;
        Self::theme_from_yaml(&text, id)
    }
}

/// Where per-user settings live: `~/Library/Application Support` on macOS, `%APPDATA%` on Windows, and
/// `$XDG_CONFIG_HOME` (or `~/.config`) elsewhere. `None` when the needed environment variables are unset.
pub fn default_support_directory() -> Option<PathBuf> {
    let env_dir = |key: &str| {
        std::env::var_os(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        env_dir("HOME").map(|home| home.join("Library/Application Support"))
    } else if cfg!(windows) {
        env_dir("APPDATA")
    } else {
        env_dir("XDG_CONFIG_HOME")
            .filter(|dir| dir.is_absolute())
            .or_else(|| env_dir("HOME").map(|home| home.join(".config")))
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn io_error(path: &Path, error: &std::io::Error) -> ThemeError {
    ThemeError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

/// Writes through a temporary file in the same folder, creating the folder first, so readers never see half a file.
fn write_atomically(path: &Path, data: &[u8]) -> Result<(), ThemeError> {
    let directory = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(directory).map_err(|error| io_error(directory, &error))?;
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(|error| io_error(directory, &error))?;
    file.write_all(data)
        .map_err(|error| io_error(path, &error))?;
    file.persist(path)
        .map_err(|error| io_error(path, &error.error))?;
    Ok(())
}

/// An approximation of Foundation's `localizedStandardCompare`: case-insensitive, with runs of digits compared by
/// value ("Theme 2" before "Theme 10").
fn standard_compare(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take_number = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.next_if(char::is_ascii_digit) {
                        digits.push(c);
                    }
                    digits
                };
                let (x, y) = (take_number(&mut a), take_number(&mut b));
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}
