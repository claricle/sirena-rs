//! Flowchart renderer: the label-free, cluster-free subset of Ruby
//! `Parser::Flowchart` + `Layout::Flowchart` (`DirectedPlacement`,
//! `FlowchartEdgeRouter`) + `Renderer::Flowchart`.
//!
//! Subgraphs are nested boxes placed level by level (`cluster`), their edges
//! routed by `router`. Anything outside the subset returns `Error::Unported`:
//! edge labels, styling/click/class statements, comments, markdown/html/quoted
//! text, long links, self loops, redeclared nodes, other node shapes.
#[allow(dead_code)]
mod place;
mod layout;
mod model;
mod parse;
mod render;
mod subgraph;
mod cluster;
mod router;

use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use layout::*;
use model::*;
use parse::*;
use std::collections::HashMap;
pub use render::render;

pub(super) fn unported(why: &str) -> Error {
    Error::Unported(format!("flowchart: {why}"))
}


/// Ruby `Float#round(nd)` (round-half-up with Ruby's correction step).
pub(super) fn rr(x: f64, nd: i32) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let binexp = crate::js_math::binexp(x);
    let limit = if binexp > 0 { binexp / 4 } else { binexp / 3 - 1 };
    if nd >= 15 - limit {
        return x;
    }
    let under = if binexp > 0 { binexp / 3 + 1 } else { binexp / 4 };
    if nd < -under {
        return 0.0;
    }
    let s = 10f64.powi(nd);
    let mut f = (x * s).round();
    if x > 0.0 {
        if (f + 0.5) / s <= x {
            f += 1.0;
        }
    } else if (f - 0.5) / s >= x {
        f -= 1.0;
    }
    f / s
}

