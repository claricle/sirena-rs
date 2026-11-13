//! Port of the elkrb 2.0.1 layered layout paths etassia exercises.
//! Interface (fixed; recorded by golden/elk_dump.rb):
//!   `layout(input)` takes the graph Hash etassia hands to `Elkrb.layout`
//!   (JSON, string keys) and returns the projection etassia reads back:
//!   `{id,x,y,width,height,children:[..],edges:[{id,sections:[{startPoint,
//!   endPoint,bendPoints}]}]}`. Anything not ported returns `Err(Unported)`.
//!
//! Layout: `model` (graph), `options` (resolved layoutOptions), `engine`
//! (algorithm dispatch), `layered` (the phase pipeline; `placement` and
//! `routing` are the hook points of the elk-place / elk-route cards),
//! `java_random`, `geometry`.
use crate::Error;
use serde_json::Value;

pub mod engine;
pub mod geometry;
pub mod java_random;
pub mod layered;
pub mod model;
pub mod options;

pub fn layout(input: &Value) -> Result<Value, Error> {
    let mut graph = model::Graph::from_json(input)?;
    engine::layout(&mut graph)?;
    Ok(graph.to_json())
}

/// Like `layout` but stops before edge routing: the returned projection has
/// node and graph geometry only (edges are left without sections).
pub fn layout_placement(input: &Value) -> Result<Value, Error> {
    let mut graph = model::Graph::from_json(input)?;
    engine::layout_placement(&mut graph)?;
    Ok(graph.to_json())
}
