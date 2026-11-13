//! Ported from `Glasshouse/Tests/TerminalThemeTests/TerminalThemeTests.swift`.

// Wrapped in a cfg(test) module so clippy treats helpers as test code (allow-unwrap-in-tests).
#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use terminal_theme::{
        AppearanceConfig, ContrastLevel, CursorSettings, CursorShape, FontSettings,
        ResolvedAppearance, TerminalTheme, ThemeChoice, ThemeColor, ThemeError, ThemeImporter,
        ThemeStore, WindowSettings,
    };

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn read_fixture(name: &str) -> String {
        fs::read_to_string(fixture(name)).expect("fixture should be readable")
    }

    fn hex(value: &str) -> ThemeColor {
        ThemeColor::from_hex(value).expect("valid colour")
    }

    fn built_in(id: &str) -> TerminalTheme {
        ThemeStore::built_in_themes()
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .expect("built-in theme")
    }

    /// A throwaway Application Support folder.
    struct Sandbox {
        root: tempfile::TempDir,
        store: ThemeStore,
    }

    impl Sandbox {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("temp dir");
            let store = ThemeStore::new(
                root.path().join("Themes"),
                root.path().join("appearance.yaml"),
            );
            Self { root, store }
        }

        fn write(&self, text: &str, path: &str) {
            let path = self.root.path().join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
            fs::write(path, text).expect("write");
        }
    }

    // MARK: Colours

    #[test]
    fn parses_and_writes_hex() {
        for (input, output) in [
            ("#1e1f22", "#1e1f22"),
            ("1E1F22", "#1e1f22"),
            ("#abc", "#aabbcc"),
            ("#11223380", "#11223380"),
        ] {
            assert_eq!(hex(input).hex(), output, "{input}");
        }
    }

    #[test]
    fn refuses_what_is_not_a_colour() {
        for input in ["", "#12", "#12345", "#gggggg", "red", "#1234567"] {
            assert_eq!(
                ThemeColor::from_hex(input),
                Err(ThemeError::InvalidColor(input.into())),
                "{input}"
            );
        }
    }

    #[test]
    fn measures_contrast_and_mixes() {
        assert!((ThemeColor::BLACK.contrast_ratio(&ThemeColor::WHITE) - 21.0).abs() < 0.01);
        assert!(ThemeColor::BLACK.is_dark() && !ThemeColor::WHITE.is_dark());
        assert_eq!(
            ThemeColor::BLACK.mixed(&ThemeColor::WHITE, 0.5).hex(),
            "#808080"
        );
    }

    #[test]
    fn clamps_components_and_writes_alpha_only_when_translucent() {
        assert_eq!(
            ThemeColor::with_alpha(2.0, -1.0, f64::NAN, 1.0).hex(),
            "#ff0000"
        );
        assert_eq!(
            ThemeColor::with_alpha(0.0, 0.0, 0.0, 0.5).hex(),
            "#00000080"
        );
        assert_eq!(hex(" #fff ").hex(), "#ffffff");
    }

    // MARK: Bundled templates

    #[test]
    fn all_fifteen_load_with_unique_ids_and_names() {
        let themes = ThemeStore::built_in_themes();
        assert_eq!(themes.len(), 15);
        assert_eq!(
            themes.iter().map(|t| &t.id).collect::<HashSet<_>>().len(),
            themes.len()
        );
        assert_eq!(
            themes.iter().map(|t| &t.name).collect::<HashSet<_>>().len(),
            themes.len()
        );
        assert!(
            themes.iter().any(|t| t.id == "glasshouse-dark")
                && themes.iter().any(|t| t.id == "glasshouse-light")
        );
    }

    #[test]
    fn every_embedded_theme_file_parses_and_validates() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        let mut count = 0;
        for entry in fs::read_dir(directory).expect("themes folder") {
            let path = entry.expect("entry").path();
            let id = path
                .file_stem()
                .expect("stem")
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(&path).expect("read");
            let theme =
                ThemeStore::theme_from_yaml(&text, &id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(theme.validated().as_ref(), Ok(&theme), "{id}");
            assert!(ThemeStore::is_built_in(&id), "{id} is not embedded");
            count += 1;
        }
        assert_eq!(count, 15);
    }

    #[test]
    fn built_in_themes_are_sorted_by_name() {
        let names: Vec<String> = ThemeStore::built_in_themes()
            .iter()
            .map(|t| t.name.to_lowercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn every_template_is_readable_and_labelled_correctly() {
        for theme in ThemeStore::built_in_themes() {
            theme.validated().expect("valid");
            assert_eq!(theme.colors.ansi().len(), 16, "{}", theme.id);
            // Some themes sit just under WCAG's 4.5 by design (Solarized Light is 4.1); the app's APCA floor raises the
            // text contrast when a theme loads.
            assert!(
                theme
                    .colors
                    .foreground
                    .contrast_ratio(&theme.colors.background)
                    >= 4.0,
                "{}",
                theme.id
            );
            let expected = if theme.colors.background.is_dark() {
                terminal_theme::Appearance::Dark
            } else {
                terminal_theme::Appearance::Light
            };
            assert_eq!(theme.appearance, expected, "{}", theme.id);
        }
    }

    #[test]
    fn survives_a_write_and_read_back() {
        for theme in ThemeStore::built_in_themes() {
            let text = ThemeStore::yaml(theme).expect("yaml");
            assert_eq!(
                ThemeStore::theme_from_yaml(&text, &theme.id).as_ref(),
                Ok(theme),
                "{}",
                theme.id
            );
        }
    }

    #[test]
    fn writes_built_in_templates_back_byte_for_byte() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        for theme in ThemeStore::built_in_themes() {
            let original =
                fs::read_to_string(directory.join(format!("{}.yaml", theme.id))).expect("read");
            assert_eq!(
                ThemeStore::yaml(theme).expect("yaml"),
                original,
                "{}",
                theme.id
            );
        }
    }

    #[test]
    fn matches_the_colours_glasshouse_used_before() {
        let dark = built_in("glasshouse-dark");
        assert_eq!(dark.colors.background.hex(), "#16181d");
        assert_eq!(dark.colors.cursor, None);
        assert_eq!(dark.colors.normal, TerminalTheme::GLASSHOUSE_NORMAL);
        assert_eq!(dark.colors.bright, TerminalTheme::GLASSHOUSE_BRIGHT);
    }

    // MARK: YAML format

    #[test]
    fn a_minimal_theme_gets_defaults() {
        let theme =
            ThemeStore::theme_from_yaml(&read_fixture("minimal.yaml"), "minimal").expect("theme");
        assert_eq!(theme.name, "Minimal");
        assert_eq!(theme.appearance, terminal_theme::Appearance::Dark);
        assert_eq!(theme.schema, 1);
        assert_eq!(theme.colors.normal, TerminalTheme::GLASSHOUSE_NORMAL);
        assert_eq!(
            theme.colors.bright,
            TerminalTheme::GLASSHOUSE_NORMAL.mixed(&ThemeColor::WHITE, 0.25)
        );
        assert!(
            theme.font.is_none()
                && theme.cursor.is_none()
                && theme.window.is_none()
                && theme.contrast.is_none()
        );
    }

    #[test]
    fn writes_kebab_case_keys_and_optional_sections() {
        let mut theme = built_in("dracula");
        theme.font = Some(FontSettings {
            family: Some("JetBrains Mono".into()),
            size: Some(14.0),
            bold_is_bright: Some(false),
        });
        theme.cursor = Some(CursorSettings {
            style: Some(CursorShape::Bar),
            blink: Some(true),
        });
        theme.window = Some(WindowSettings {
            padding_horizontal: Some(16.0),
            padding_vertical: Some(8.0),
            opacity: Some(0.9),
        });
        theme.contrast = Some(ContrastLevel::Faithful);
        let text = ThemeStore::yaml(&theme).expect("yaml");
        for key in [
            "cursor-text:",
            "bold-is-bright: false",
            "size: 14\n",
            "padding-horizontal: 16\n",
            "opacity: 0.9\n",
            "style: bar",
            "contrast: faithful",
        ] {
            assert!(text.contains(key), "{key} in:\n{text}");
        }
        assert!(!text.contains("id:"));
        assert_eq!(ThemeStore::theme_from_yaml(&text, "dracula"), Ok(theme));
    }

    #[test]
    fn explains_what_is_wrong() {
        assert_eq!(
            ThemeStore::theme_from_yaml("name: X\n", "x"),
            Err(ThemeError::MissingKey("colors".into()))
        );
        assert_eq!(
            ThemeStore::theme_from_yaml(
                "name: X\ncolors: {background: blue, foreground: '#fff'}\n",
                "x"
            ),
            Err(ThemeError::InvalidColor("blue".into()))
        );
        assert_eq!(
            ThemeStore::theme_from_yaml(
                "name: X\ncolors: {background: '#000', foreground: '#fff'}\nfont: {size: 100}\n",
                "x"
            ),
            Err(ThemeError::OutOfRange("font size".into(), 100.0))
        );
        assert_eq!(
            ThemeStore::theme_from_yaml(
                "schema: 2\nname: X\ncolors: {background: '#000', foreground: '#fff'}\n",
                "x"
            ),
            Err(ThemeError::UnsupportedSchema(2))
        );
        assert_eq!(
            ThemeStore::theme_from_yaml(
                "name: ''\ncolors: {background: '#000', foreground: '#fff'}\n",
                "x"
            ),
            Err(ThemeError::InvalidName(String::new()))
        );
    }

    #[test]
    fn reports_nested_missing_keys_and_unreadable_files() {
        assert_eq!(
            ThemeStore::theme_from_yaml(
                "name: X\ncolors: {background: '#000', foreground: '#fff', normal: {black: '#000'}}\n",
                "x"
            ),
            Err(ThemeError::MissingKey("colors.normal.red".into()))
        );
        assert_eq!(
            ThemeStore::theme_from_yaml("[1, 2]", "x"),
            Err(ThemeError::Unreadable("x".into()))
        );
        assert_eq!(
            ThemeStore::theme_from_yaml(
                "name: X\ncolors: {background: '#000', foreground: '#fff'}\nappearance: dim\n",
                ""
            ),
            Err(ThemeError::Unreadable("The file".into()))
        );
    }

    #[test]
    fn error_messages_match_the_swift_app() {
        assert_eq!(
            ThemeError::OutOfRange("font size".into(), 100.0).to_string(),
            "100.0 is out of range for the font size."
        );
        assert_eq!(
            ThemeError::InvalidColor("blue".into()).to_string(),
            "“blue” isn't a colour. Use #rrggbb, for example #1e1f22."
        );
        assert_eq!(
            ThemeError::MissingKey("colors".into()).to_string(),
            "The theme has no “colors”, which every theme needs."
        );
    }

    // MARK: Appearance settings

    #[test]
    fn theme_settings_win_over_global_ones_which_win_over_defaults() {
        let mut theme = built_in("nord");
        let config = AppearanceConfig {
            font: FontSettings {
                family: Some("Menlo".into()),
                size: Some(12.0),
                bold_is_bright: None,
            },
            window: WindowSettings {
                opacity: Some(0.8),
                ..Default::default()
            },
            contrast: ContrastLevel::Faithful,
            ..Default::default()
        };
        let resolved = ResolvedAppearance::new(theme.clone(), &config);
        assert!(
            resolved.font_family == "Menlo"
                && resolved.font_size == 12.0
                && resolved.bold_is_bright
        );
        assert!(
            resolved.opacity == 0.8
                && resolved.padding_horizontal == 10.0
                && resolved.contrast == ContrastLevel::Faithful
        );
        assert!(resolved.cursor_shape == CursorShape::Block && !resolved.cursor_blink);

        theme.font = Some(FontSettings {
            size: Some(15.0),
            ..Default::default()
        });
        theme.contrast = Some(ContrastLevel::High);
        let resolved = ResolvedAppearance::new(theme, &config);
        assert!(
            resolved.font_family == "Menlo"
                && resolved.font_size == 15.0
                && resolved.contrast == ContrastLevel::High
        );
    }

    #[test]
    fn picks_the_theme_for_the_system_appearance() {
        let choice = ThemeChoice {
            dark: "dracula".into(),
            light: "solarized-light".into(),
            follow_system: true,
        };
        assert_eq!(choice.theme_id(true), "dracula");
        assert_eq!(choice.theme_id(false), "solarized-light");
        let fixed = ThemeChoice {
            dark: "dracula".into(),
            light: "x".into(),
            follow_system: false,
        };
        assert_eq!(fixed.theme_id(false), "dracula");
    }

    #[test]
    fn saves_loads_and_falls_back() {
        let sandbox = Sandbox::new();
        assert_eq!(sandbox.store.load_config(), AppearanceConfig::default());

        let mut config = AppearanceConfig::default();
        config.theme.dark = "dracula".into();
        config.font.size = Some(15.0);
        sandbox.store.save_config(&config).expect("save");
        assert_eq!(sandbox.store.load_config(), config);
        let written = fs::read_to_string(&sandbox.store.config_path).expect("read");
        assert!(written.contains("follow-system: true"), "{written}");
        assert!(written.starts_with(
            "# Glasshouse appearance settings. Settings › Appearance writes this file.\n"
        ));

        sandbox.write("theme: { dark: nord }\n", "appearance.yaml");
        assert_eq!(sandbox.store.load_config().theme.dark, "nord");
        assert_eq!(sandbox.store.load_config().theme.light, "glasshouse-light");

        sandbox.write("font: { size: 999 }\n", "appearance.yaml");
        assert_eq!(sandbox.store.load_config(), AppearanceConfig::default());

        config.font.size = Some(999.0);
        assert!(sandbox.store.save_config(&config).is_err());
    }

    #[test]
    fn resolves_a_missing_theme_to_glasshouses() {
        let sandbox = Sandbox::new();
        let mut config = AppearanceConfig::default();
        config.theme.dark = "deleted-theme".into();
        assert_eq!(
            sandbox.store.resolved(true, Some(&config)).theme.id,
            "glasshouse-dark"
        );
        assert_eq!(
            sandbox.store.resolved(false, Some(&config)).theme.id,
            "glasshouse-light"
        );
    }

    // MARK: Your themes

    #[test]
    fn saves_with_an_id_from_the_name_and_never_overwrites_a_template() {
        let sandbox = Sandbox::new();
        let store = &sandbox.store;
        let dracula = built_in("dracula");

        let copy = store.duplicate(&dracula, "My Dracula!").expect("duplicate");
        assert_eq!(copy.id, "my-dracula");
        assert!(copy.author.is_none() && copy.source.is_none());
        assert_eq!(store.theme("my-dracula").as_ref(), Some(&copy));

        let mut edited = dracula.clone();
        edited.colors.background = hex("#000000");
        let saved = store.save(&edited).expect("save");
        assert_eq!(saved.id, "dracula-2");
        assert_eq!(
            store
                .theme("dracula")
                .map(|t| t.colors.background.hex())
                .as_deref(),
            Some("#282a36")
        );

        assert_eq!(store.all_themes().len(), 17);
        assert_eq!(
            store.delete("dracula"),
            Err(ThemeError::ReadOnly("dracula".into()))
        );
        store.delete("dracula-2").expect("delete");
        assert!(store.theme("dracula-2").is_none());
        assert_eq!(
            store.delete("../x"),
            Err(ThemeError::NotFound("../x".into()))
        );
    }

    #[test]
    fn lists_files_that_do_not_load_instead_of_failing() {
        let sandbox = Sandbox::new();
        sandbox.write("name: Broken\n", "Themes/broken.yaml");
        sandbox.write(
            "name: Spoof\ncolors: {background: '#000', foreground: '#fff'}\n",
            "Themes/dracula.yaml",
        );
        sandbox.write(&read_fixture("minimal.yaml"), "Themes/minimal.yaml");

        let ids: Vec<String> = sandbox
            .store
            .user_themes()
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["minimal"]);
        let files: Vec<String> = sandbox
            .store
            .user_theme_problems()
            .into_iter()
            .map(|p| p.file)
            .collect();
        assert_eq!(files, ["broken.yaml", "dracula.yaml"]);
        assert_eq!(
            sandbox.store.theme("dracula").map(|t| t.name).as_deref(),
            Some("Dracula")
        );
    }

    #[test]
    fn unique_ids_are_slugged_and_capped() {
        let sandbox = Sandbox::new();
        assert_eq!(
            sandbox.store.unique_id("  ¡Héllo,  World!  "),
            "h-llo-world"
        );
        assert_eq!(sandbox.store.unique_id("!!!"), "theme");
        assert_eq!(sandbox.store.unique_id(&"a".repeat(80)).len(), 56);
        assert_eq!(sandbox.store.unique_id("Nord"), "nord-2");
        assert_eq!(sandbox.store.unique_name("Nord"), "Nord 2");
        assert!(
            ThemeStore::is_valid_id("my-theme-2")
                && !ThemeStore::is_valid_id("-x")
                && !ThemeStore::is_valid_id("A")
        );
    }

    // MARK: Importing

    #[test]
    fn reads_iterm2_colors() {
        let data = fs::read(fixture("Sample.itermcolors")).expect("fixture");
        let theme = ThemeImporter::iterm(&data, "Sample").expect("theme");
        assert_eq!(theme.colors.background.hex(), "#1e1e1e");
        assert_eq!(theme.colors.foreground.hex(), "#cccccc");
        assert_eq!(
            theme.colors.selection.map(|c| c.hex()).as_deref(),
            Some("#264f78")
        );
        assert_eq!(theme.colors.normal.red.hex(), "#cd3131");
        assert_eq!(theme.colors.bright.white.hex(), "#ffffff");
        assert_eq!(theme.appearance, terminal_theme::Appearance::Dark);
    }

    #[test]
    fn reads_ghostty_themes_and_fills_gaps() {
        let theme =
            ThemeImporter::ghostty(&read_fixture("sample-ghostty"), "Tomorrow").expect("theme");
        assert_eq!(theme.colors.background.hex(), "#1d1f21");
        assert_eq!(
            theme.colors.cursor.map(|c| c.hex()).as_deref(),
            Some("#c5c8c6")
        );
        assert_eq!(theme.colors.normal.blue.hex(), "#81a2be");
        assert_eq!(theme.colors.bright.red.hex(), "#d54e53");
        assert_eq!(
            theme.colors.bright.black,
            theme.colors.normal.black.mixed(&ThemeColor::WHITE, 0.25)
        );
    }

    #[test]
    fn refuses_files_it_does_not_understand() {
        assert_eq!(
            ThemeImporter::ghostty("hello = world\n", "x"),
            Err(ThemeError::UnknownFormat("x".into()))
        );
        assert_eq!(
            ThemeImporter::iterm(b"not a plist", "x"),
            Err(ThemeError::UnknownFormat("x".into()))
        );
    }

    #[test]
    fn imports_into_your_themes_without_clashing_names() {
        let sandbox = Sandbox::new();
        let first = sandbox
            .store
            .import_theme(&fixture("Sample.itermcolors"))
            .expect("first");
        let second = sandbox
            .store
            .import_theme(&fixture("Sample.itermcolors"))
            .expect("second");
        assert!(first.name == "Sample" && first.id == "sample");
        assert!(second.name == "Sample 2" && second.id == "sample-2");
        assert_eq!(
            sandbox
                .store
                .import_theme(&fixture("minimal.yaml"))
                .expect("minimal")
                .id,
            "minimal"
        );
    }
}
