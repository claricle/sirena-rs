//! Internal graph model the layered phases work on (elkrb `Graph::Graph`,
//! `Graph::Node`, `Graph::Edge`, restricted to what etassia sends: a flat
//! level of sized nodes joined by single-source single-target edges).
//! elk-place and elk-route extend this: positions are written onto
//! `Graph::children`, routes onto `Edge::sections`.
use super::geometry::Section;
use super::options::Options;
use crate::Error;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

fn unported(what: impl Into<String>) -> Error {
    Error::Unported(format!("elk {}", what.into()))
}

fn validation(message: impl Into<String>) -> Error {
    Error::Parse { class: "ValidationError".into(), message: message.into(), line: 0, col: 0 }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub options: Options,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub id: Option<String>,
    pub sources: Vec<String>,
    pub targets: Vec<String>,
    /// Raw label Hashes. The layered phases never read them (labels are
    /// placed after layout and are not part of the projection).
    pub labels: Vec<Value>,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone)]
pub struct Graph {
    pub id: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub options: Options,
    pub children: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Resolves ids to the child that owns them, for one level (elkrb
/// `NodeIndex`; etassia's levels have no ports or nested nodes, so a node
/// owns only its own id).
#[derive(Debug, Clone)]
pub struct NodeIndex {
    by_id: HashMap<String, usize>,
}

impl NodeIndex {
    pub fn build(graph: &Graph) -> Result<NodeIndex, Error> {
        let mut by_id = HashMap::new();
        for (i, c) in graph.children.iter().enumerate() {
            if by_id.insert(c.id.clone(), i).is_some() {
                return Err(validation(format!("duplicate id: {}", c.id)));
            }
        }
        Ok(NodeIndex { by_id })
    }

    /// Index into `Graph::children` of the node owning `id`.
    pub fn owner(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }
}

fn opt_f64(m: &Map<String, Value>, key: &str) -> Result<Option<f64>, Error> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().map(Some).ok_or_else(|| unported(format!("{key} = {v}"))),
    }
}

fn empty_list(m: &Map<String, Value>, key: &str) -> Result<(), Error> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(a)) if a.is_empty() => Ok(()),
        Some(_) => Err(unported(format!("non-empty {key}"))),
    }
}

fn str_list(v: Option<&Value>, what: &str) -> Result<Vec<String>, Error> {
    match v {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(a)) => a
            .iter()
            .map(|e| e.as_str().map(String::from).ok_or_else(|| unported(format!("{what} entry {e}"))))
            .collect(),
        Some(o) => Err(unported(format!("{what} {o}"))),
    }
}

impl Node {
    fn from_json(v: &Value) -> Result<Node, Error> {
        let m = v.as_object().ok_or_else(|| unported("node that is not a Hash"))?;
        for k in m.keys() {
            match k.as_str() {
                "id" | "width" | "height" | "layoutOptions" | "labels" | "ports" | "children" | "edges" => {}
                other => return Err(unported(format!("node key {other}"))),
            }
        }
        for k in ["labels", "ports", "children", "edges"] {
            empty_list(m, k)?;
        }
        let id = match m.get("id") {
            Some(Value::String(s)) => s.clone(),
            None | Some(Value::Null) => return Err(validation("node without id")),
            Some(o) => return Err(unported(format!("node id {o}"))),
        };
        Ok(Node {
            id,
            x: None,
            y: None,
            width: opt_f64(m, "width")?,
            height: opt_f64(m, "height")?,
            options: Options::from_json(m.get("layoutOptions"))?,
        })
    }
}

impl Edge {
    fn from_json(v: &Value) -> Result<Edge, Error> {
        let m = v.as_object().ok_or_else(|| unported("edge that is not a Hash"))?;
        for k in m.keys() {
            match k.as_str() {
                "id" | "sources" | "targets" | "labels" | "metadata" => {}
                other => return Err(unported(format!("edge key {other}"))),
            }
        }
        let labels = match m.get("labels") {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(a)) => a.clone(),
            Some(o) => return Err(unported(format!("edge labels {o}"))),
        };
        let id = match m.get("id") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(o) => return Err(unported(format!("edge id {o}"))),
        };
        let sources = str_list(m.get("sources"), "sources")?;
        let targets = str_list(m.get("targets"), "targets")?;
        // layered raises for a missing endpoint or a hyperedge.
        if sources.len() != 1 || targets.len() != 1 {
            return Err(unported("edge that is not one source and one target"));
        }
        Ok(Edge { id, sources, targets, labels, sections: vec![] })
    }
}

impl Graph {
    pub fn from_json(v: &Value) -> Result<Graph, Error> {
        let m = v.as_object().ok_or_else(|| unported("graph that is not a Hash"))?;
        for k in m.keys() {
            match k.as_str() {
                "id" | "children" | "edges" | "layoutOptions" => {}
                other => return Err(unported(format!("graph key {other}"))),
            }
        }
        let id = match m.get("id") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(o) => return Err(unported(format!("graph id {o}"))),
        };
        let list = |k: &str| match m.get(k) {
            None | Some(Value::Null) => Ok(&[][..]),
            Some(Value::Array(a)) => Ok(a.as_slice()),
            Some(o) => Err(unported(format!("{k} {o}"))),
        };
        Ok(Graph {
            id,
            width: None,
            height: None,
            options: Options::from_json(m.get("layoutOptions"))?,
            children: list("children")?.iter().map(Node::from_json).collect::<Result<_, _>>()?,
            edges: list("edges")?.iter().map(Edge::from_json).collect::<Result<_, _>>()?,
        })
    }

    /// The projection etassia reads back (recorded by golden/elk_dump.rb).
    pub fn to_json(&self) -> Value {
        let num = |x: Option<f64>| x.map_or(Value::Null, |f| json!(f));
        let pt = |p: &super::geometry::Point| json!({"x": p.x, "y": p.y});
        let children: Vec<Value> = self
            .children
            .iter()
            .map(|n| {
                json!({"id": n.id, "x": num(n.x), "y": num(n.y), "width": num(n.width),
                       "height": num(n.height), "children": [], "edges": []})
            })
            .collect();
        let edges: Vec<Value> = self
            .edges
            .iter()
            .map(|e| {
                let sections: Vec<Value> = e
                    .sections
                    .iter()
                    .map(|s| {
                        json!({"startPoint": pt(&s.start_point), "endPoint": pt(&s.end_point),
                               "bendPoints": s.bend_points.iter().map(pt).collect::<Vec<_>>()})
                    })
                    .collect();
                json!({"id": e.id, "sections": sections})
            })
            .collect();
        json!({"id": self.id, "x": null, "y": null, "width": num(self.width),
               "height": num(self.height), "children": children, "edges": edges})
    }
}
