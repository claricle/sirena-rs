//! Stage 2: SVG tree, serializer, theme and number formatters.
//!
//! Stable API for the stage-4 renderers:
//! - [`fmt_num`] — JS `Number#toString` (`Etassia::JsNumber.stringify`).
//! - [`ruby_float`] / [`write_num`] / [`round4`] / [`read_num`] — Ruby `Float#to_s`,
//!   `Numbers.write`, `Float#round(4)`, `Numbers.read`.
//! - [`Elem`] builder + [`Document`] + [`serialize`] — byte-identical to Ruby `Svg::*#to_xml`.
//! - [`escape_text`] / [`escape_attr`] — XML escaping.
//! - [`Theme`] — built-in themes.
//!
//! Shared helpers: [`Style`], [`Arrowhead`] (+[`ArrowPath`]), [`PathGeometry`] (+[`Anchor`]).

mod arrowhead;
mod escape;
mod node;
mod number;
mod path_geometry;
mod style;
mod theme;
mod theme_data;

pub use arrowhead::{ArrowPath, Arrowhead};
pub use escape::{escape_attr, escape_text, valid_name};
pub use node::{serialize, Document, Elem};
pub use number::{fmt_num, read_num, round4, ruby_float, write_num};
pub use path_geometry::{Anchor, PathGeometry};
pub use style::Style;
pub use theme::Theme;
