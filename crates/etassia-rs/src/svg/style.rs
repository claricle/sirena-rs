//! Port of `Etassia::Svg::Style`: CSS properties for an inline `style` value.

use super::number::ruby_float;

/// CSS styling for an SVG element. Unset fields are omitted from [`Style::to_css`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub stroke_width: Option<f64>,
    pub stroke_dasharray: Option<String>,
    pub opacity: Option<f64>,
    pub fill_opacity: Option<f64>,
    pub stroke_opacity: Option<f64>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_weight: Option<String>,
    pub text_anchor: Option<String>,
}

impl Style {
    /// Ruby `Style#to_css`: `prop:value` pairs joined by `;`, in declaration order.
    /// Floats print as Ruby `Float#to_s` (`2.0`).
    pub fn to_css(&self) -> String {
        let f = |v: &Option<f64>| v.map(ruby_float);
        let pairs: [(&str, Option<String>); 11] = [
            ("fill", self.fill.clone()),
            ("stroke", self.stroke.clone()),
            ("stroke-width", f(&self.stroke_width)),
            ("stroke-dasharray", self.stroke_dasharray.clone()),
            ("opacity", f(&self.opacity)),
            ("fill-opacity", f(&self.fill_opacity)),
            ("stroke-opacity", f(&self.stroke_opacity)),
            ("font-family", self.font_family.clone()),
            ("font-size", f(&self.font_size)),
            ("font-weight", self.font_weight.clone()),
            ("text-anchor", self.text_anchor.clone()),
        ];
        pairs
            .into_iter()
            .filter_map(|(p, v)| v.map(|v| format!("{p}:{v}")))
            .collect::<Vec<_>>()
            .join(";")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_empty_string() {
        assert_eq!(Style::default().to_css(), "");
    }

    #[test]
    fn declaration_order_and_float_format() {
        let s = Style {
            font_size: Some(14.0),
            fill: Some("#333".into()),
            stroke_width: Some(1.5),
            opacity: Some(0.5),
            text_anchor: Some("middle".into()),
            ..Default::default()
        };
        // Ruby: fill:#333;stroke-width:1.5;opacity:0.5;font-size:14.0;text-anchor:middle
        assert_eq!(s.to_css(), "fill:#333;stroke-width:1.5;opacity:0.5;font-size:14.0;text-anchor:middle");
    }
}
