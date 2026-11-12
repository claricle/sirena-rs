//! Sequence renderer: port of `Parser::Builders::Sequence` (model rules),
//! `Layout::Sequence` (geometry) and `Renderer::Sequence`.
//!
//! Declined to the Ruby path (`Error::Unported`): a participant with an empty
//! id, and every statement list that Ruby's builder rejects (a `create` of a
//! known id, a `create`/`destroy` without its message, deactivating an
//! inactive participant).
mod frames;
mod layout;
mod model;
mod num;
mod text;

use crate::Error;

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "sequence")?;
    let diagram = model::build(&tree, text::global_wrap(source))?;
    let scene = layout::scene(&diagram);
    Ok(scene.draw())
}
