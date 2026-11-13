//! Builds the engine's `TerminalAppearance` from the chosen theme, with the APCA floor applied at load time.
//! Because the grid is opaque, the effective background is known exactly and the contrast check is deterministic.

use design_system::{ContrastPolicy, Rgba, apca};
use terminal_core::{TerminalAppearance, TerminalCursorShape};
use terminal_theme::{
    AppearanceConfig, ContrastLevel, CursorShape, ResolvedAppearance, ThemeColor, ThemeStore,
};

/// Black/white slots are exempt: they are *meant* to match or invert the background.
const EXEMPT_SLOTS: [usize; 4] = [0, 7, 8, 15];

/// The macOS accent blue, for themes without a cursor colour (`.controlAccentColor` in the Swift app).
const ACCENT: Rgba = Rgba::rgb(0.0, 0.478, 1.0);

/// System accessibility settings that override the theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
    pub increase_contrast: bool,
    pub reduce_transparency: bool,
}

/// Increased Contrast always wins; otherwise the user's contrast level picks the policy.
pub fn contrast_policy(level: ContrastLevel, increase_contrast: bool) -> ContrastPolicy {
    if increase_contrast {
        return ContrastPolicy::INCREASED;
    }
    match level {
        ContrastLevel::Standard => ContrastPolicy::STANDARD,
        ContrastLevel::Faithful => ContrastPolicy::THEME_FAITHFUL,
        ContrastLevel::High => ContrastPolicy::INCREASED,
    }
}

pub fn rgba(c: ThemeColor) -> Rgba {
    Rgba::new(c.red as f32, c.green as f32, c.blue as f32, c.alpha as f32)
}

fn shape(shape: CursorShape) -> TerminalCursorShape {
    match shape {
        CursorShape::Block => TerminalCursorShape::Block,
        CursorShape::Underline => TerminalCursorShape::Underline,
        CursorShape::Bar => TerminalCursorShape::Bar,
    }
}

/// The background the contrast floor is held against. Opaque, it is the theme's. Translucent, the
/// window shows whatever is behind it (wallpaper, desktop), so the floor is held against the worst
/// case: the background at `opacity` over white when the text is lighter, over black when darker.
pub fn contrast_background(bg: Rgba, fg: Rgba, opacity: f32) -> Rgba {
    if opacity >= 1.0 {
        return bg;
    }
    let behind = if apca::luminance(fg) > apca::luminance(bg) {
        Rgba::WHITE
    } else {
        Rgba::BLACK
    };
    bg.mix(behind, 1.0 - opacity.clamp(0.0, 1.0))
}

/// The engine appearance for `resolved`, with the contrast floor and accessibility overrides applied.
pub fn terminal_appearance(
    resolved: &ResolvedAppearance,
    a11y: Accessibility,
) -> TerminalAppearance {
    let policy = contrast_policy(resolved.contrast, a11y.increase_contrast);
    let colors = &resolved.theme.colors;
    let opaque = |c: ThemeColor| rgba(c).with_alpha(1.0);
    let bg = opaque(colors.background);

    // Translucency is opt-in and forced off by the accessibility settings.
    let opacity = if a11y.reduce_transparency || a11y.increase_contrast {
        1.0
    } else {
        resolved.opacity as f32
    };
    let fg = opaque(colors.foreground);
    let floor_bg = contrast_background(bg, fg, opacity);

    let ansi: [Rgba; 16] = std::array::from_fn(|i| {
        let c = opaque(colors.ansi()[i]);
        if EXEMPT_SLOTS.contains(&i) {
            c
        } else {
            apca::enforce(policy.palette_minimum, c, floor_bg)
        }
    });
    let foreground = apca::enforce(policy.foreground_minimum, fg, floor_bg);

    let mut appearance =
        TerminalAppearance::new(foreground, bg, colors.cursor.map_or(ACCENT, rgba), ansi);
    appearance.font_name.clone_from(&resolved.font_family);
    appearance.font_size = resolved.font_size as f32;
    appearance.cursor_text = colors.cursor_text.map(rgba);
    appearance.selection = colors.selection.map(rgba);
    appearance.cursor_shape = shape(resolved.cursor_shape);
    appearance.cursor_blink = resolved.cursor_blink;
    appearance.bold_is_bright = resolved.bold_is_bright;
    appearance.bold = colors
        .bold
        .map(|c| apca::enforce(policy.foreground_minimum, opaque(c), floor_bg));
    appearance.background_opacity = opacity;
    appearance.padding_horizontal = resolved.padding_horizontal as f32;
    appearance.padding_vertical = resolved.padding_vertical as f32;
    appearance
}

/// The themes and `appearance.yaml`, shared by Settings and every terminal session.
pub struct ThemeLibrary {
    pub store: Option<ThemeStore>,
    pub themes: Vec<terminal_theme::TerminalTheme>,
    pub problems: Vec<terminal_theme::ThemeFileProblem>,
    pub config: AppearanceConfig,
    /// Bumped on every change so sessions know to re-apply.
    pub generation: u64,
}

impl ThemeLibrary {
    pub fn load() -> Self {
        let mut library = Self::with_store(ThemeStore::standard());
        library.reload();
        library
    }

    /// Built-in themes only, nothing read from or written to disk (snapshot scenes and tests).
    pub fn built_in() -> Self {
        let mut library = Self::with_store(None);
        library.themes = ThemeStore::built_in_themes().to_vec();
        library
    }

    fn with_store(store: Option<ThemeStore>) -> Self {
        ThemeLibrary {
            store,
            themes: Vec::new(),
            problems: Vec::new(),
            config: AppearanceConfig::default(),
            generation: 0,
        }
    }

    pub fn reload(&mut self) {
        match &self.store {
            Some(store) => {
                self.themes = store.all_themes();
                self.problems = store.user_theme_problems();
                self.config = store.load_config();
            }
            None => self.themes = ThemeStore::built_in_themes().to_vec(),
        }
        self.generation += 1;
    }

    pub fn theme(&self, id: &str) -> Option<&terminal_theme::TerminalTheme> {
        self.themes.iter().find(|t| t.id == id)
    }

    pub fn resolved(&self, dark: bool) -> ResolvedAppearance {
        let fallback = if dark {
            "glasshouse-dark"
        } else {
            "glasshouse-light"
        };
        let chosen = self
            .theme(self.config.theme.theme_id(dark))
            .or_else(|| self.theme(fallback))
            .or_else(|| self.themes.first());
        match chosen {
            Some(theme) => ResolvedAppearance::new(theme.clone(), &self.config),
            None => ResolvedAppearance::new(ThemeStore::emergency_theme(), &self.config),
        }
    }

    /// Changes `appearance.yaml`. An invalid change is not saved, and the error says why.
    pub fn update_config(
        &mut self,
        change: impl FnOnce(&mut AppearanceConfig),
    ) -> Result<(), String> {
        let mut updated = self.config.clone();
        change(&mut updated);
        let updated = updated.validated().map_err(|e| e.to_string())?;
        if let Some(store) = &self.store {
            store.save_config(&updated).map_err(|e| e.to_string())?;
        }
        self.config = updated;
        self.generation += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(id: &str) -> ResolvedAppearance {
        let library = ThemeLibrary::built_in();
        let theme = library.theme(id).cloned().expect("built-in theme");
        ResolvedAppearance::new(theme, &AppearanceConfig::default())
    }

    #[test]
    fn contrast_policy_should_force_increased_when_system_contrast_is_on() {
        assert_eq!(
            contrast_policy(ContrastLevel::Faithful, true),
            ContrastPolicy::INCREASED
        );
    }

    #[test]
    fn contrast_policy_should_map_faithful_to_theme_faithful() {
        assert_eq!(
            contrast_policy(ContrastLevel::Faithful, false),
            ContrastPolicy::THEME_FAITHFUL
        );
    }

    #[test]
    fn terminal_appearance_should_lift_palette_colours_to_the_floor() {
        let r = resolved("glasshouse-dark");
        let a = terminal_appearance(&r, Accessibility::default());
        let floor = ContrastPolicy::STANDARD.palette_minimum;
        for (i, c) in a
            .ansi
            .iter()
            .enumerate()
            .filter(|(i, _)| !EXEMPT_SLOTS.contains(i))
        {
            assert!(
                apca::contrast(*c, a.background).abs() >= floor - 0.5,
                "slot {i}"
            );
        }
    }

    #[test]
    fn terminal_appearance_should_leave_exempt_slots_untouched() {
        let r = resolved("glasshouse-dark");
        let a = terminal_appearance(&r, Accessibility::default());
        assert_eq!(a.ansi[0], rgba(r.theme.colors.ansi()[0]).with_alpha(1.0));
    }

    #[test]
    fn terminal_appearance_should_force_opaque_with_reduce_transparency() {
        let mut r = resolved("glasshouse-dark");
        r.opacity = 0.6;
        let a11y = Accessibility {
            increase_contrast: false,
            reduce_transparency: true,
        };
        assert_eq!(terminal_appearance(&r, a11y).background_opacity, 1.0);
    }

    #[test]
    fn translucent_text_should_meet_the_floor_over_the_brightest_backdrop() {
        let mut r = resolved("glasshouse-dark");
        r.opacity = 0.7;
        let a = terminal_appearance(&r, Accessibility::default());
        let worst = contrast_background(a.background, a.foreground, 0.7);
        let floor = ContrastPolicy::STANDARD;
        assert!(apca::contrast(a.foreground, worst).abs() >= floor.foreground_minimum - 0.5);
        for (i, c) in a
            .ansi
            .iter()
            .enumerate()
            .filter(|(i, _)| !EXEMPT_SLOTS.contains(i))
        {
            assert!(
                apca::contrast(*c, worst).abs() >= floor.palette_minimum - 0.5,
                "slot {i}"
            );
        }
    }

    #[test]
    fn contrast_background_should_be_the_theme_background_when_opaque() {
        let bg = Rgba::rgb(0.1, 0.1, 0.12);
        assert_eq!(contrast_background(bg, Rgba::WHITE, 1.0), bg);
    }

    #[test]
    fn resolved_should_fall_back_to_glasshouse_theme_for_unknown_id() {
        let mut library = ThemeLibrary::built_in();
        library.config.theme.dark = "no-such-theme".into();
        assert_eq!(library.resolved(true).theme.id, "glasshouse-dark");
    }
}
