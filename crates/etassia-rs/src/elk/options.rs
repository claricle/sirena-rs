//! The layout options sirena passes, resolved against elkrb's registry
//! defaults (lib/elkrb/options/registry.rb). A graph or node reads only its
//! own `layoutOptions` (no inheritance), as `Options::Resolver#get` does.
//! An option key or value outside what sirena uses is `Unported`.
use crate::Error;
use serde_json::Value;

fn unported(what: impl Into<String>) -> Error {
    Error::Unported(format!("elk option {}", what.into()))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub algorithm: String,
    /// UNDEFINED, RIGHT, LEFT, DOWN or UP (registry default RIGHT).
    pub direction: String,
    /// AUTOMATIC, LEFT, RIGHT, TOP, BOTTOM or CENTER.
    pub alignment: String,
    /// LAYER_SWEEP, MEDIAN_LAYER_SWEEP, INTERACTIVE or NONE.
    pub crossing_strategy: String,
    /// NONE, NODES_AND_EDGES, PREFER_EDGES or PREFER_NODES.
    pub model_order: String,
    pub separate_components: bool,
    pub node_node: f64,
    pub layer_spacing: f64,
    pub hierarchy_handling: String,
    pub random_seed: i64,
    pub padding: Padding,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            algorithm: "layered".into(),
            direction: "RIGHT".into(),
            alignment: "AUTOMATIC".into(),
            crossing_strategy: "LAYER_SWEEP".into(),
            model_order: "NONE".into(),
            separate_components: true,
            node_node: 20.0,
            layer_spacing: 20.0,
            hierarchy_handling: "INHERIT".into(),
            random_seed: 1,
            padding: Padding { left: 12.0, top: 12.0, right: 12.0, bottom: 12.0 },
        }
    }
}

fn en(key: &str, v: &Value, allowed: &[&str]) -> Result<String, Error> {
    match v.as_str() {
        Some(s) if allowed.contains(&s) => Ok(s.to_string()),
        _ => Err(unported(format!("{key} = {v}"))),
    }
}

fn num(key: &str, v: &Value) -> Result<f64, Error> {
    match v.as_f64() {
        Some(f) if f.is_finite() => Ok(f),
        _ => Err(unported(format!("{key} = {v}"))),
    }
}

impl Options {
    /// Resolves a `layoutOptions` Hash (None = absent) over the defaults.
    pub fn from_json(opts: Option<&Value>) -> Result<Options, Error> {
        let mut o = Options::default();
        let map = match opts {
            None | Some(Value::Null) => return Ok(o),
            Some(Value::Object(m)) => m,
            Some(other) => return Err(unported(format!("layoutOptions {other}"))),
        };
        for (k, v) in map {
            match k.as_str() {
                "elk.algorithm" => o.algorithm = en(k, v, &["layered"])?,
                "elk.direction" => {
                    let d = en(k, v, &["UNDEFINED", "RIGHT", "LEFT", "DOWN", "UP", "HORIZONTAL", "VERTICAL"])?;
                    o.direction = match d.as_str() {
                        "HORIZONTAL" => "RIGHT".into(),
                        "VERTICAL" => "DOWN".into(),
                        _ => d,
                    };
                }
                "elk.alignment" => {
                    o.alignment = en(k, v, &["AUTOMATIC", "LEFT", "RIGHT", "TOP", "BOTTOM", "CENTER"])?
                }
                "elk.layered.crossingMinimization.strategy" => {
                    o.crossing_strategy = en(k, v, &["LAYER_SWEEP", "MEDIAN_LAYER_SWEEP", "INTERACTIVE", "NONE"])?
                }
                "elk.layered.considerModelOrder.strategy" => {
                    o.model_order = en(k, v, &["NONE", "NODES_AND_EDGES", "PREFER_EDGES", "PREFER_NODES"])?
                }
                "elk.separateConnectedComponents" => match v.as_bool() {
                    Some(b) => o.separate_components = b,
                    None => return Err(unported(format!("{k} = {v}"))),
                },
                "elk.spacing.nodeNode" => o.node_node = num(k, v)?,
                "elk.layered.spacing.nodeNodeBetweenLayers" => o.layer_spacing = num(k, v)?,
                "elk.hierarchyHandling" => {
                    o.hierarchy_handling = en(k, v, &["INHERIT", "INCLUDE_CHILDREN", "SEPARATE_CHILDREN"])?
                }
                other => return Err(unported(other)),
            }
        }
        Ok(o)
    }
}
