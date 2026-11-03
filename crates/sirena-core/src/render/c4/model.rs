//! `Parser::Builders::C4`: the statement model, read from the native tree.
use super::{plain_object, present, text};
use crate::parse::Tree;
use serde_json::{Map, Value};

#[derive(Default, Clone)]
pub struct Element {
    pub id: String,
    pub label: Option<String>,
    pub element_type: String,
    pub description: Option<String>,
    pub technology: Option<String>,
    pub external: bool,
    pub boundary_id: Option<String>,
}

#[derive(Default, Clone)]
pub struct Boundary {
    pub id: String,
    pub label: Option<String>,
    pub boundary_type: String,
    pub type_param: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Default, Clone)]
pub struct Relationship {
    pub from: Option<String>,
    pub to: Option<String>,
    pub label: Option<String>,
    pub technology: Option<String>,
    pub rel_type: String,
}

#[derive(Default)]
pub struct Diagram {
    pub title: Option<String>,
    pub layout_config: Option<String>,
    pub elements: Vec<Element>,
    pub boundaries: Vec<Boundary>,
    pub relationships: Vec<Relationship>,
}

type Stmt = Option<Map<String, Value>>;

const STOP_KEYS: [&str; 6] = ["element_type", "boundary_type", "rel_type", "title", "config_params", "header"];

fn has(m: &Map<String, Value>, k: &str) -> bool {
    present(m.get(k))
}

fn merge_scattered(items: Vec<Stmt>) -> Vec<Stmt> {
    let mut out = vec![];
    let mut i = 0;
    while i < items.len() {
        let mut item = items[i].clone();
        i += 1;
        let scattered = item.as_ref().is_some_and(|m| (has(m, "element_type") || has(m, "boundary_type")) && !has(m, "body"));
        if scattered {
            while let Some(Some(next)) = items.get(i) {
                if STOP_KEYS.iter().any(|k| has(next, k)) {
                    break;
                }
                if let Some(m) = item.as_mut() {
                    for (k, v) in next {
                        m.insert(k.clone(), v.clone());
                    }
                }
                i += 1;
            }
        }
        out.push(item);
    }
    out
}

fn extract_text(v: &Value) -> String {
    let s = match plain_object(v) {
        Some(o) => {
            if let Some(s) = o.get("string").filter(|v| !v.is_null()) {
                if s.is_array() { String::new() } else { text(s) }
            } else if let Some(var) = o.get("var").filter(|v| !v.is_null()) {
                text(var)
            } else {
                o.values().next().map(text).unwrap_or_default()
            }
        }
        None => text(v),
    };
    super::strip(&s).to_string()
}

fn type_name(v: &Value) -> String {
    match plain_object(v).and_then(|o| o.get("variable")).filter(|v| !v.is_null()) {
        Some(var) => extract_text(&var["var"]),
        None => text(v),
    }
}

fn field(m: &Map<String, Value>, k: &str) -> Option<String> {
    m.get(k).filter(|v| !v.is_null()).map(extract_text)
}

struct Builder {
    d: Diagram,
    current: Option<String>,
    ok: bool,
}

impl Builder {
    fn statement(&mut self, m: &Map<String, Value>) {
        if has(m, "header") {
        } else if has(m, "title") {
            self.d.title = Some(extract_text(&m["title"]));
        } else if has(m, "config_params") {
            self.d.layout_config = Some(config_parts(&m["config_params"]).join(", "));
        } else if has(m, "boundary_type") {
            self.boundary(m);
        } else if has(m, "rel_type") {
            self.d.relationships.push(Relationship {
                rel_type: text(&m["rel_type"]),
                from: field(m, "from"),
                to: field(m, "to"),
                label: field(m, "label"),
                technology: field(m, "technology"),
            });
        } else if has(m, "element_type") {
            self.element(m);
        }
    }

    fn boundary(&mut self, m: &Map<String, Value>) -> Option<String> {
        let mut b = Boundary { boundary_type: type_name(&m["boundary_type"]), ..Default::default() };
        match field(m, "id") {
            Some(id) => b.id = id,
            None => self.ok = false,
        }
        b.label = field(m, "label");
        b.type_param = field(m, "type");
        b.parent_id = self.current.clone();
        let id = b.id.clone();
        self.d.boundaries.push(b);
        if let Some(body) = m.get("body").filter(|v| !v.is_null()) {
            let old = self.current.replace(id.clone());
            for stmt in nested(body).into_iter().flatten() {
                if has(&stmt, "boundary_type") {
                    self.boundary(&stmt);
                } else if has(&stmt, "element_type") {
                    self.element(&stmt);
                }
            }
            self.current = old;
        }
        Some(id)
    }

    fn element(&mut self, m: &Map<String, Value>) {
        let mut e = Element { element_type: type_name(&m["element_type"]), boundary_id: self.current.clone(), ..Default::default() };
        match field(m, "id") {
            Some(id) => e.id = id,
            None => self.ok = false,
        }
        e.label = field(m, "label");
        e.description = field(m, "description");
        e.technology = field(m, "technology");
        e.external = e.element_type.ends_with("_Ext");
        self.d.elements.push(e);
    }
}

fn config_parts(params: &Value) -> Vec<String> {
    let part = |p: &Value, require: bool| -> Option<String> {
        let key = p.get("key").filter(|v| !v.is_null()).map(extract_text);
        let value = p.get("value").filter(|v| !v.is_null()).map(extract_text);
        if require && (key.is_none() || value.is_none()) {
            return None;
        }
        Some(format!("{}={}", key.unwrap_or_default(), value.unwrap_or_default()))
    };
    match params {
        Value::Array(a) => a.iter().filter_map(|p| part(p, true)).collect(),
        v if plain_object(v).is_some() => part(v, false).into_iter().collect(),
        _ => vec![],
    }
}

fn flatten<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| flatten(x, out)),
        x => out.push(x),
    }
}

fn nested(body: &Value) -> Vec<Stmt> {
    let list: Vec<&Value> = match body {
        Value::Array(a) => a.iter().collect(),
        v => vec![v],
    };
    let mut items: Vec<Stmt> = vec![];
    for it in list {
        if let Some(inner) = plain_object(it).and_then(|o| o.get("item")).filter(|v| !v.is_null()) {
            let mut flat = vec![];
            flatten(inner, &mut flat);
            items.extend(flat.into_iter().map(|v| plain_object(v).cloned()));
        }
    }
    merge_scattered(items)
}

pub fn build(tree: &Tree) -> Option<Diagram> {
    let items: Vec<Stmt> = match tree {
        Value::Array(a) => merge_scattered(a.iter().map(|v| plain_object(v).cloned()).collect()),
        v => vec![plain_object(v).cloned()],
    };
    let mut b = Builder { d: Diagram::default(), current: None, ok: true };
    for m in items.into_iter().flatten() {
        b.statement(&m);
    }
    b.ok.then_some(b.d)
}
