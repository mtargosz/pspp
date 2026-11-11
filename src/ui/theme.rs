//! btop "Default" theme colors, 101-step gradients, the 256-color fallback, and btop `.theme`
//! file loading.

use std::fs;
use std::path::Path;

use ratatui::style::Color;

pub const GRADIENT_STEPS: usize = 101;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn grey(value: u8) -> Self {
        Self(value, value, value)
    }

    fn to_indexed(self) -> u8 {
        let Self(r, g, b) = self;
        if r == g && g == b {
            return match r {
                0..=7 => 16,
                249..=255 => 231,
                _ => 232 + ((u16::from(r) - 8) * 24 / 247).min(23) as u8,
            };
        }
        let level = |v: u8| ((u16::from(v) + 25) / 51) as u8;
        16 + 36 * level(r) + 6 * level(g) + level(b)
    }
}

/// Builds the btop gradient: 50 linear steps start→mid, then 50 mid→end.
pub fn build_gradient(start: Rgb, mid: Rgb, end: Rgb) -> [Rgb; GRADIENT_STEPS] {
    let lerp = |a: u8, b: u8, step: i32| -> u8 {
        let (a, b) = (i32::from(a), i32::from(b));
        let value = a * 50 + (b - a) * step;
        ((value + 25).div_euclid(50)).clamp(0, 255) as u8
    };
    let mix = |from: Rgb, to: Rgb, step: i32| {
        Rgb(
            lerp(from.0, to.0, step),
            lerp(from.1, to.1, step),
            lerp(from.2, to.2, step),
        )
    };

    let mut gradient = [start; GRADIENT_STEPS];
    for (pct, slot) in gradient.iter_mut().enumerate() {
        *slot = if pct <= 50 {
            mix(start, mid, pct as i32)
        } else {
            mix(mid, end, pct as i32 - 50)
        };
    }
    gradient
}

/// True when `COLORTERM` advertises 24-bit color.
pub fn truecolor_from_env(colorterm: Option<&str>) -> bool {
    colorterm
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "truecolor" || v == "24bit"
        })
        .unwrap_or(false)
}

/// `truecolor` option: `auto` follows `COLORTERM`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruecolorMode {
    Auto,
    On,
    Off,
}

impl TruecolorMode {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "on" | "true" => Self::On,
            "off" | "false" => Self::Off,
            _ => Self::Auto,
        }
    }

    pub fn as_config(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn cycle(self, delta: isize) -> Self {
        const ORDER: [TruecolorMode; 3] =
            [TruecolorMode::Auto, TruecolorMode::On, TruecolorMode::Off];
        let current = ORDER.iter().position(|mode| *mode == self).unwrap_or(0);
        ORDER[(current as isize + delta).rem_euclid(ORDER.len() as isize) as usize]
    }

    pub fn resolve(self, colorterm: Option<&str>) -> bool {
        match self {
            Self::Auto => truecolor_from_env(colorterm),
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// Colours a btop `.theme` file may override; `None` keeps the built-in Default value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeSpec {
    pub main_bg: Option<Rgb>,
    pub main_fg: Option<Rgb>,
    pub title: Option<Rgb>,
    pub hi_fg: Option<Rgb>,
    pub selected_bg: Option<Rgb>,
    pub selected_fg: Option<Rgb>,
    pub inactive_fg: Option<Rgb>,
    pub meter_bg: Option<Rgb>,
    pub div_line: Option<Rgb>,
    pub gradient_start: Option<Rgb>,
    pub gradient_mid: Option<Rgb>,
    pub gradient_end: Option<Rgb>,
}

/// Parses a btop colour: `#RRGGBB`, `#RR` (grey) or `R G B` decimals. Empty/invalid → `None`.
pub fn parse_theme_color(value: &str) -> Option<Rgb> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        return match hex.len() {
            2 => u8::from_str_radix(hex, 16).ok().map(Rgb::grey),
            6 => Some(Rgb(
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            )),
            _ => None,
        };
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() == 3 {
        return Some(Rgb(
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ));
    }
    None
}

/// Parses btop `.theme` text (`theme[key]="value"`); unknown keys, comments and bad values are
/// skipped so a damaged theme degrades to the Default colours instead of failing.
pub fn parse_btop_theme(text: &str) -> ThemeSpec {
    let mut spec = ThemeSpec::default();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("theme[") else {
            continue;
        };
        let Some((key, value)) = rest.split_once("]=") else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        let color = parse_theme_color(value);
        match key.trim() {
            "main_bg" => spec.main_bg = color,
            "main_fg" => spec.main_fg = color,
            "title" => spec.title = color,
            "hi_fg" => spec.hi_fg = color,
            "selected_bg" => spec.selected_bg = color,
            "selected_fg" => spec.selected_fg = color,
            "inactive_fg" => spec.inactive_fg = color,
            "meter_bg" => spec.meter_bg = color,
            "div_line" => spec.div_line = color,
            "cpu_start" => spec.gradient_start = color,
            "cpu_mid" => spec.gradient_mid = color,
            "cpu_end" => spec.gradient_end = color,
            _ => {}
        }
    }
    spec
}

/// Theme names available in `dir` (`*.theme` file stems), sorted, `Default` first.
pub fn discover_themes(dir: Option<&Path>) -> Vec<String> {
    let mut names: Vec<String> = dir
        .and_then(|dir| fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("theme") {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_owned();
            crate::config::is_valid_theme_name(&stem).then_some(stem)
        })
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names.retain(|name| name != crate::config::DEFAULT_THEME_NAME);
    names.insert(0, crate::config::DEFAULT_THEME_NAME.to_owned());
    names
}

/// Loads `<dir>/<name>.theme`; `None` for `Default`, missing, unreadable or invalid names.
pub fn load_theme_spec(dir: Option<&Path>, name: &str) -> Option<ThemeSpec> {
    if name == crate::config::DEFAULT_THEME_NAME || !crate::config::is_valid_theme_name(name) {
        return None;
    }
    let text = fs::read_to_string(dir?.join(format!("{name}.theme"))).ok()?;
    Some(parse_btop_theme(&text))
}

#[derive(Debug, Clone)]
pub struct Theme {
    /// `None` keeps the terminal background (btop `theme_background=false`).
    pub main_bg: Option<Rgb>,
    pub main_fg: Rgb,
    pub title: Rgb,
    pub hi_fg: Rgb,
    pub selected_bg: Rgb,
    pub selected_fg: Rgb,
    pub inactive_fg: Rgb,
    pub meter_bg: Rgb,
    pub div_line: Rgb,
    truecolor: bool,
    usage_gradient: [Rgb; GRADIENT_STEPS],
}

impl Theme {
    /// Detects truecolor support once from `COLORTERM`.
    pub fn detect() -> Self {
        Self::new(truecolor_from_env(
            std::env::var("COLORTERM").ok().as_deref(),
        ))
    }

    pub fn new(truecolor: bool) -> Self {
        Self::build(truecolor, true, None)
    }

    /// Builds a theme: Default colours overlaid with `spec`. The theme's `main_bg` is only
    /// painted when `theme_background` is on (off keeps the terminal background).
    pub fn build(truecolor: bool, theme_background: bool, spec: Option<&ThemeSpec>) -> Self {
        let blank = ThemeSpec::default();
        let spec = spec.unwrap_or(&blank);
        let start = spec.gradient_start.unwrap_or(Rgb(0x77, 0xca, 0x9b));
        let mid = spec.gradient_mid.unwrap_or(Rgb(0xcb, 0xc0, 0x6c));
        let end = spec.gradient_end.unwrap_or(Rgb(0xdc, 0x4c, 0x4c));
        Self {
            main_bg: spec.main_bg.filter(|_| theme_background),
            main_fg: spec.main_fg.unwrap_or(Rgb::grey(0xcc)),
            title: spec.title.unwrap_or(Rgb::grey(0xee)),
            hi_fg: spec.hi_fg.unwrap_or(Rgb(0xb5, 0x40, 0x40)),
            selected_bg: spec.selected_bg.unwrap_or(Rgb(0x6a, 0x2f, 0x2f)),
            selected_fg: spec.selected_fg.unwrap_or(Rgb::grey(0xee)),
            inactive_fg: spec.inactive_fg.unwrap_or(Rgb::grey(0x40)),
            meter_bg: spec.meter_bg.unwrap_or(Rgb::grey(0x40)),
            div_line: spec.div_line.unwrap_or(Rgb::grey(0x30)),
            truecolor,
            usage_gradient: build_gradient(start, mid, end),
        }
    }

    /// Applies the truecolor / 256-color policy chosen at construction.
    pub fn color(&self, rgb: Rgb) -> Color {
        if self.truecolor {
            Color::Rgb(rgb.0, rgb.1, rgb.2)
        } else {
            Color::Indexed(rgb.to_indexed())
        }
    }

    pub fn usage_rgb(&self, pct: u8) -> Rgb {
        self.usage_gradient[usize::from(pct.min(100))]
    }

    pub fn usage_color(&self, pct: u8) -> Color {
        self.color(self.usage_rgb(pct))
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::detect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_btop_color_syntaxes() {
        assert_eq!(parse_theme_color("#77ca9b"), Some(Rgb(0x77, 0xca, 0x9b)));
        assert_eq!(parse_theme_color("#cc"), Some(Rgb::grey(0xcc)));
        assert_eq!(parse_theme_color("10 20 30"), Some(Rgb(10, 20, 30)));
        for bad in [
            "",
            "#",
            "#12",
            "#12345",
            "#gg0000",
            "1 2",
            "1 2 300",
            "#é1é1é1",
        ] {
            let expected = if bad == "#12" {
                Some(Rgb::grey(0x12))
            } else {
                None
            };
            assert_eq!(parse_theme_color(bad), expected, "{bad}");
        }
    }

    const SAMPLE_THEME: &str = "# comment\n\
        theme[main_bg]=\"#101010\"\n\
        theme[main_fg]=\"#aa\"\n\
        theme[title]=\"200 100 50\"\n\
        theme[cpu_start]=\"#000000\"\n\
        theme[cpu_end]=\"#ffffff\"\n\
        theme[hi_fg]=\"not-a-color\"\n\
        theme[proc_misc]=\"#123456\"\n\
        garbage line\n";

    #[test]
    fn parses_btop_theme_files_tolerantly() {
        let spec = parse_btop_theme(SAMPLE_THEME);
        assert_eq!(spec.main_bg, Some(Rgb(0x10, 0x10, 0x10)));
        assert_eq!(spec.main_fg, Some(Rgb::grey(0xaa)));
        assert_eq!(spec.title, Some(Rgb(200, 100, 50)));
        assert_eq!(spec.hi_fg, None, "invalid colour is ignored");
        assert_eq!(spec.gradient_mid, None);
        assert_eq!(parse_btop_theme("\u{0}\u{1}garbage"), ThemeSpec::default());
    }

    #[test]
    fn theme_build_overlays_spec_and_honours_theme_background() {
        let spec = parse_btop_theme(SAMPLE_THEME);
        let themed = Theme::build(true, true, Some(&spec));
        assert_eq!(themed.main_bg, Some(Rgb(0x10, 0x10, 0x10)));
        assert_eq!(themed.main_fg, Rgb::grey(0xaa));
        assert_eq!(
            themed.hi_fg,
            Theme::new(true).hi_fg,
            "unset keys keep Default"
        );
        assert_eq!(themed.usage_rgb(0), Rgb(0, 0, 0));
        assert_eq!(themed.usage_rgb(100), Rgb(255, 255, 255));
        assert_eq!(
            themed.usage_rgb(50),
            Rgb(0xcb, 0xc0, 0x6c),
            "mid falls back"
        );

        let transparent = Theme::build(true, false, Some(&spec));
        assert_eq!(transparent.main_bg, None);
        assert_eq!(Theme::build(true, true, None).main_bg, None);
    }

    #[test]
    fn discovers_and_loads_theme_files() {
        let dir = std::env::temp_dir().join(format!("pspp-themes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("themes dir");
        fs::write(dir.join("Zeta.theme"), SAMPLE_THEME).expect("write");
        fs::write(dir.join("alpha.theme"), "").expect("write");
        fs::write(dir.join("notes.txt"), "x").expect("write");
        fs::write(dir.join("Default.theme"), "x").expect("write");

        assert_eq!(
            discover_themes(Some(&dir)),
            vec!["Default", "alpha", "Zeta"]
        );
        assert_eq!(discover_themes(None), vec!["Default"]);
        assert!(load_theme_spec(Some(&dir), "Zeta").is_some());
        assert!(load_theme_spec(Some(&dir), "Default").is_none());
        assert!(load_theme_spec(Some(&dir), "missing").is_none());
        assert!(load_theme_spec(Some(&dir), "../Zeta").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truecolor_mode_resolves_and_cycles() {
        assert!(TruecolorMode::Auto.resolve(Some("truecolor")));
        assert!(!TruecolorMode::Auto.resolve(None));
        assert!(TruecolorMode::On.resolve(None));
        assert!(!TruecolorMode::Off.resolve(Some("truecolor")));
        assert_eq!(TruecolorMode::from_config("ON"), TruecolorMode::On);
        assert_eq!(TruecolorMode::from_config("bogus"), TruecolorMode::Auto);
        assert_eq!(TruecolorMode::Auto.cycle(1), TruecolorMode::On);
        assert_eq!(TruecolorMode::Auto.cycle(-1), TruecolorMode::Off);
    }

    #[test]
    fn gradient_has_101_anchored_entries() {
        let theme = Theme::new(true);
        assert_eq!(theme.usage_gradient.len(), 101);
        assert_eq!(theme.usage_rgb(0), Rgb(0x77, 0xca, 0x9b));
        assert_eq!(theme.usage_rgb(50), Rgb(0xcb, 0xc0, 0x6c));
        assert_eq!(theme.usage_rgb(100), Rgb(0xdc, 0x4c, 0x4c));
    }

    #[test]
    fn gradient_red_is_monotonic_and_green_falls_after_mid() {
        let theme = Theme::new(true);
        for pct in 1..=100u8 {
            assert!(theme.usage_rgb(pct).0 >= theme.usage_rgb(pct - 1).0);
        }
        for pct in 51..=100u8 {
            assert!(theme.usage_rgb(pct).1 <= theme.usage_rgb(pct - 1).1);
        }
    }

    #[test]
    fn usage_color_clamps_above_100() {
        let theme = Theme::new(true);
        assert_eq!(theme.usage_color(250), theme.usage_color(100));
    }

    #[test]
    fn truecolor_flag_is_honored() {
        let rgb = Rgb(0x77, 0xca, 0x9b);
        assert_eq!(Theme::new(true).color(rgb), Color::Rgb(0x77, 0xca, 0x9b));
        assert!(matches!(Theme::new(false).color(rgb), Color::Indexed(_)));
    }

    #[test]
    fn colorterm_detection() {
        assert!(truecolor_from_env(Some("truecolor")));
        assert!(truecolor_from_env(Some("24bit")));
        assert!(!truecolor_from_env(Some("256color")));
        assert!(!truecolor_from_env(None));
    }

    #[test]
    fn indexed_mapping_spot_checks() {
        assert_eq!(Rgb(0, 0, 0).to_indexed(), 16);
        assert_eq!(Rgb(255, 255, 255).to_indexed(), 231);
        // Cube path: pure red / green / blue.
        assert_eq!(Rgb(255, 0, 0).to_indexed(), 196);
        assert_eq!(Rgb(0, 255, 0).to_indexed(), 46);
        assert_eq!(Rgb(0, 0, 255).to_indexed(), 21);
        // #dc4c4c: round(4.31)=4, round(1.49)=1 → 16 + 144 + 6 + 1.
        assert_eq!(Rgb(0xdc, 0x4c, 0x4c).to_indexed(), 167);
        // Greys use the 232+ ramp.
        assert_eq!(Rgb::grey(0x80).to_indexed(), 243);
        assert!((232..=255).contains(&Rgb::grey(0x40).to_indexed()));
    }
}
