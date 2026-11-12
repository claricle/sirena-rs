//! Built-in themes (generated from the Ruby YAML; see `gen_theme.rb`).

use super::theme_data;

/// Static theme tables. Keys are `"section.name"`, e.g. `"colors.background"`.
#[derive(Debug)]
pub struct ThemeData {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) strings: &'static [(&'static str, &'static str)],
    pub(super) numbers: &'static [(&'static str, f64)],
    pub(super) bools: &'static [(&'static str, bool)],
    pub(super) palettes: &'static [(&'static str, &'static [&'static str])],
}

/// A built-in theme: `Theme::get("default")`, `.color("background")`,
/// `.palette("flow_palette", i)`.
#[derive(Debug, Clone, Copy)]
pub struct Theme(&'static ThemeData);

impl Theme {
    /// Looks up a built-in theme (`default`, `light`, `dark`, `high_contrast`).
    pub fn get(name: &str) -> Option<Theme> {
        theme_data::ALL.iter().find(|t| t.name == name).map(|t| Theme(t))
    }

    /// The `default` theme (what Ruby uses when no theme is given).
    pub fn default_theme() -> Theme {
        Theme::get("default").expect("builtin default theme")
    }

    /// Theme name.
    pub fn name(&self) -> &'static str {
        self.0.name
    }

    /// Theme description.
    pub fn description(&self) -> &'static str {
        self.0.description
    }

    /// A colour from `colors.*` (e.g. `"node_fill"`).
    pub fn color(&self, key: &str) -> Option<&'static str> {
        self.string("colors", key)
    }

    /// A string value from any section (`typography.font_family`, `shapes.dash_pattern_dashed`, ...).
    pub fn string(&self, section: &str, key: &str) -> Option<&'static str> {
        let full = format!("{section}.{key}");
        self.0.strings.iter().find(|(k, _)| *k == full).map(|(_, v)| *v)
    }

    /// A numeric value (`typography.font_size_normal`, `spacing.node_margin`, ...).
    pub fn number(&self, section: &str, key: &str) -> Option<f64> {
        let full = format!("{section}.{key}");
        self.0.numbers.iter().find(|(k, _)| *k == full).map(|(_, v)| *v)
    }

    /// A boolean value (`effects.shadow_enabled`, ...).
    pub fn flag(&self, section: &str, key: &str) -> Option<bool> {
        let full = format!("{section}.{key}");
        self.0.bools.iter().find(|(k, _)| *k == full).map(|(_, v)| *v)
    }

    /// Colour `index % len` from a categorical palette (`"flow_palette"`, `"pie_palette"`, ...).
    pub fn palette(&self, name: &str, index: usize) -> Option<&'static str> {
        let p = self.0.palettes.iter().find(|(k, _)| *k == name)?.1;
        if p.is_empty() { None } else { Some(p[index % p.len()]) }
    }
}
