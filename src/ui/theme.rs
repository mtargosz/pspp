//! btop "Default" theme colors, 101-step gradients and the 256-color fallback.

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
        let start = Rgb(0x77, 0xca, 0x9b);
        let mid = Rgb(0xcb, 0xc0, 0x6c);
        let end = Rgb(0xdc, 0x4c, 0x4c);
        let usage_gradient = build_gradient(start, mid, end);
        Self {
            main_bg: None,
            main_fg: Rgb::grey(0xcc),
            title: Rgb::grey(0xee),
            hi_fg: Rgb(0xb5, 0x40, 0x40),
            selected_bg: Rgb(0x6a, 0x2f, 0x2f),
            selected_fg: Rgb::grey(0xee),
            inactive_fg: Rgb::grey(0x40),
            meter_bg: Rgb::grey(0x40),
            div_line: Rgb::grey(0x30),
            truecolor,
            usage_gradient,
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
