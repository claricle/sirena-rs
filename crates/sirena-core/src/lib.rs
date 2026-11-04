//! sirena-core: Rust runtime for Sirena. Unported types return
//! `Error::Unported` so callers can route to the Ruby fallback.
pub mod elk;
pub mod error;
pub mod parse;
pub mod js_math;
pub mod render;
pub mod svg;
pub mod text;

pub use error::Error;
pub use parse::detect;
pub use render::render;
