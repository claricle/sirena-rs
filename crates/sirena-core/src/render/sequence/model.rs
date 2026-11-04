//! `Parser::Builders::Sequence`: the parse tree to the diagram model.
use super::text;
use crate::render::pie::{items, strip, text_of};
use crate::Error;
use serde_json::Value;
use std::collections::HashSet;

fn unported() -> Error {
    Error::Unported("sequence".into())
}

pub struct Participant {
    pub id: String,
    pub label: String,
    pub actor: bool,
    pub created: bool,
    pub shape: Option<String>,
}

pub struct Message {
    pub from: String,
    pub to: String,
    /// `message_text` as written (the stripped capture, not decoded).
    pub text: String,
    pub line_style: &'static str,
    pub head_style: &'static str,
    pub head_side: &'static str,
}

pub struct Note {
    pub text: String,
    pub position: &'static str,
    pub participant_ids: Vec<String>,
    pub message_index: usize,
    pub order: usize,
}

pub struct Section {
    pub label: String,
    pub start: usize,
    pub order: usize,
}

pub struct Frame {
    pub kind: &'static str,
    pub label: String,
    pub start: usize,
    pub stop: usize,
    pub depth: usize,
    pub open_order: usize,
    pub close_order: usize,
    pub sections: Vec<Section>,
}

pub struct BoxGroup {
    pub color: Option<String>,
    pub title: String,
    pub members: Vec<String>,
}

pub struct Diagram {
    pub participants: Vec<Participant>,
    pub messages: Vec<Message>,
    pub notes: Vec<Note>,
    pub frames: Vec<Frame>,
    pub boxes: Vec<BoxGroup>,
    pub wrap: bool,
}

const SOLID: &str = "solid";
const DOTTED: &str = "dotted";

fn arrow_style(base: &str) -> (&'static str, &'static str, &'static str) {
    match base {
        "->" => (SOLID, "none", "target"),
        "-->" => (DOTTED, "none", "target"),
        "->>" => (SOLID, "filled", "target"),
        "-->>" => (DOTTED, "filled", "target"),
        "-x" | "-X" => (SOLID, "cross", "target"),
        "--x" | "--X" => (DOTTED, "cross", "target"),
        "-)" => (SOLID, "open", "target"),
        "--)" => (DOTTED, "open", "target"),
        "-|/" => (SOLID, "half_bottom", "target"),
        "--|/" => (DOTTED, "half_bottom", "target"),
        "-|\\" => (SOLID, "half_top", "target"),
        "--|\\" => (DOTTED, "half_top", "target"),
        "-//" => (SOLID, "stick_bottom", "target"),
        "--//" => (DOTTED, "stick_bottom", "target"),
        "-\\\\" => (SOLID, "stick_top", "target"),
        "--\\\\" => (DOTTED, "stick_top", "target"),
        "/|-" => (SOLID, "half_bottom", "source"),
        "/|--" => (DOTTED, "half_bottom", "source"),
        "\\|-" => (SOLID, "half_top", "source"),
        "\\|--" => (DOTTED, "half_top", "source"),
        "//-" => (SOLID, "stick_bottom", "source"),
        "//--" => (DOTTED, "stick_bottom", "source"),
        "\\\\-" => (SOLID, "stick_top", "source"),
        "\\\\--" => (DOTTED, "stick_top", "source"),
        "<<->>" => (SOLID, "filled", "both"),
        "<<-->>" => (DOTTED, "filled", "both"),
        _ => (SOLID, "filled", "target"),
    }
}

/// A key whose value is present (Ruby truthiness: `[]` and `""` count).
fn has<'a>(stmt: &'a Value, key: &str) -> Option<&'a Value> {
    stmt.get(key).filter(|v| !v.is_null() && **v != Value::Bool(false))
}

/// `extract_text`.
fn extract_text(value: Option<&Value>) -> String {
    let Some(value) = value else { return String::new() };
    let raw = match value {
        v if v.is_object() && v.get("value").is_none() => {
            if let Some(s) = has(v, "string") {
                text_of(s)
            } else {
                return extract_text(v.get("message_text"));
            }
        }
        v => text_of(v),
    };
    strip(&raw).to_string()
}

fn decoded_text(value: Option<&Value>) -> String {
    text::decode(&extract_text(value))
}

fn actor_id(value: Option<&Value>) -> String {
    value.map(|v| strip(&text_of(v)).to_string()).unwrap_or_default()
}

#[derive(Default)]
struct Builder {
    d: Option<Diagram>,
    known: HashSet<String>,
    pending_destroyed: Option<String>,
    pending_created: Option<String>,
    activations: Vec<(String, Vec<bool>)>,
    event_order: usize,
    open_frames: Vec<usize>,
    box_members: Option<Vec<String>>,
}

impl Builder {
    fn diagram(&mut self) -> &mut Diagram {
        self.d.as_mut().expect("diagram")
    }

    fn find(&mut self, id: &str) -> Option<usize> {
        self.diagram().participants.iter().position(|p| p.id == id)
    }

    fn ensure(&mut self, id: &str, track: bool) {
        if track {
            self.known.insert(id.to_string());
        }
        if self.find(id).is_none() {
            self.diagram().participants.push(Participant {
                id: id.to_string(),
                label: text::decode(id),
                actor: false,
                created: false,
                shape: None,
            });
        }
    }

    fn add_participant(&mut self, stmt: &Value, actor: bool) -> Result<(), Error> {
        let id = actor_id(stmt.get("id"));
        let created = has(stmt, "create").is_some();
        let shape = has(stmt, "shape_metadata").and_then(|m| shape_type(&text_of(m)));
        self.known.insert(id.clone());
        if let Some(members) = self.box_members.as_mut() {
            members.push(id.clone());
        }
        let shown = text::decode(&id);
        let label = match has(stmt, "label") {
            Some(v) => decoded_text(Some(v)),
            None => shown.clone(),
        };
        let label = if label.is_empty() { shown } else { label };
        match self.find(&id) {
            Some(at) => {
                let p = &mut self.diagram().participants[at];
                if !label.is_empty() {
                    p.label = label;
                }
                p.actor = actor;
                if shape.is_some() {
                    p.shape = shape;
                }
            }
            None => self.diagram().participants.push(Participant { id, label, actor, created, shape }),
        }
        Ok(())
    }

    fn add_message(&mut self, stmt: &Value) -> Result<(), Error> {
        let from = actor_id(stmt.get("from"));
        let to = actor_id(stmt.get("to"));
        if let Some(created) = self.pending_created.take() {
            if created != to {
                return Err(unported());
            }
        } else if let Some(destroyed) = self.pending_destroyed.take() {
            if destroyed != from && destroyed != to {
                return Err(unported());
            }
        }
        let arrow = stmt.get("arrow").cloned().unwrap_or(Value::Null);
        let base = arrow.get("arrow_base").map(text_of).unwrap_or_default();
        let (line_style, head_style, head_side) = arrow_style(&base);
        let message_text = extract_text(stmt.get("text"));
        self.ensure(&from, true);
        self.ensure(&to, true);
        let suffix = arrow.get("activation").map(text_of).unwrap_or_default();
        match strip(&suffix) {
            "+" => self.track(&to, true)?,
            "-" => self.track(&from, false)?,
            _ => {}
        }
        self.diagram().messages.push(Message {
            from,
            to,
            text: message_text,
            line_style,
            head_style,
            head_side,
        });
        Ok(())
    }

    fn add_note(&mut self, stmt: &Value) -> Result<(), Error> {
        let captures: Vec<&Value> = match stmt.get("participants") {
            Some(Value::Array(a)) => a.iter().collect(),
            Some(v) => vec![v],
            None => vec![],
        };
        let ids: Vec<String> = captures.iter().map(|c| actor_id(c.get("participant"))).collect();
        for id in &ids {
            self.ensure(id, true);
        }
        let position = stmt.get("position").cloned().unwrap_or(Value::Null);
        let position = if has(&position, "left_of").is_some() {
            "left_of"
        } else if has(&position, "right_of").is_some() {
            "right_of"
        } else {
            "over"
        };
        let message_index = self.diagram().messages.len();
        let text = decoded_text(stmt.get("note_text"));
        let order = self.next_order();
        self.diagram().notes.push(Note { text, position, participant_ids: ids, message_index, order });
        Ok(())
    }

    /// `track_activation`; only its failure (deactivating an inactive
    /// participant) matters here.
    fn track(&mut self, id: &str, activate: bool) -> Result<(), Error> {
        self.ensure(id, false);
        let at = match self.activations.iter().position(|(k, _)| k == id) {
            Some(at) => at,
            None => {
                self.activations.push((id.to_string(), vec![]));
                self.activations.len() - 1
            }
        };
        let open = &mut self.activations[at].1;
        if activate {
            open.push(false);
            return Ok(());
        }
        match open.iter_mut().rev().find(|ended| !**ended) {
            Some(ended) => {
                *ended = true;
                Ok(())
            }
            None => Err(unported()),
        }
    }

    fn next_order(&mut self) -> usize {
        self.event_order += 1;
        self.event_order
    }

    fn statements(&mut self, list: Option<&Value>) -> Result<(), Error> {
        for stmt in items(list.unwrap_or(&Value::Null)) {
            self.statement(stmt)?;
        }
        Ok(())
    }

    fn within_frame(&mut self, kind: &'static str, stmt: &Value, sections: Option<&'static str>) -> Result<(), Error> {
        let label = decoded_text(stmt.get(format!("{kind}_label")));
        let start = self.diagram().messages.len();
        let depth = self.open_frames.len();
        let open_order = self.next_order();
        let at = self.diagram().frames.len();
        self.diagram().frames.push(Frame {
            kind,
            label,
            start,
            stop: 0,
            depth,
            open_order,
            close_order: 0,
            sections: vec![],
        });
        self.open_frames.push(at);
        self.statements(stmt.get(format!("{kind}_statements")))?;
        if let Some(section_kind) = sections {
            let blocks_key = match section_kind {
                "else" => "else_blocks",
                "and" => "and_blocks",
                _ => "option_blocks",
            };
            for block in items(stmt.get(blocks_key).unwrap_or(&Value::Null)) {
                let label = decoded_text(block.get(format!("{section_kind}_label")));
                let start = self.diagram().messages.len();
                let order = self.next_order();
                self.diagram().frames[at].sections.push(Section { label, start, order });
                self.statements(block.get(format!("{section_kind}_statements")))?;
            }
        }
        self.open_frames.pop();
        let stop = self.diagram().messages.len();
        let close_order = self.next_order();
        let frame = &mut self.diagram().frames[at];
        frame.stop = stop;
        frame.close_order = close_order;
        Ok(())
    }

    fn within_box(&mut self, stmt: &Value) -> Result<(), Error> {
        let (color, title) = box_title(&decoded_text(stmt.get("box_label")));
        self.box_members = Some(vec![]);
        self.statements(stmt.get("box_statements"))?;
        let members = self.box_members.take().unwrap_or_default();
        self.diagram().boxes.push(BoxGroup { color, title, members });
        Ok(())
    }

    fn statement(&mut self, stmt: &Value) -> Result<(), Error> {
        if has(stmt, "create").is_some() {
            let id = actor_id(stmt.get("id"));
            if self.known.contains(&id) {
                return Err(unported());
            }
            self.pending_created = Some(id);
        }
        if has(stmt, "participant").is_some() {
            self.add_participant(stmt, false)
        } else if has(stmt, "actor").is_some() {
            self.add_participant(stmt, true)
        } else if let Some(id) = has(stmt, "destroy") {
            self.pending_destroyed = Some(actor_id(Some(id)));
            Ok(())
        } else if let Some(id) = has(stmt, "links") {
            self.ensure(&actor_id(Some(id)), true);
            Ok(())
        } else if has(stmt, "from").is_some() && has(stmt, "to").is_some() && has(stmt, "arrow").is_some() {
            self.add_message(stmt)
        } else if has(stmt, "position").is_some() && has(stmt, "note_text").is_some() {
            self.add_note(stmt)
        } else if let Some(id) = has(stmt, "activate") {
            self.track(&actor_id(Some(id)), true)
        } else if let Some(id) = has(stmt, "deactivate") {
            self.track(&actor_id(Some(id)), false)
        } else if has(stmt, "box_label").is_some() {
            self.within_box(stmt)
        } else if has(stmt, "loop_label").is_some() {
            self.within_frame("loop", stmt, None)
        } else if has(stmt, "alt_label").is_some() {
            self.within_frame("alt", stmt, Some("else"))
        } else if has(stmt, "opt_label").is_some() {
            self.within_frame("opt", stmt, None)
        } else if has(stmt, "rect_label").is_some() {
            self.within_frame("rect", stmt, None)
        } else if has(stmt, "par_label").is_some() {
            self.within_frame("par", stmt, Some("and"))
        } else if has(stmt, "critical_label").is_some() {
            self.within_frame("critical", stmt, Some("option"))
        } else if has(stmt, "break_label").is_some() {
            self.within_frame("break", stmt, None)
        } else {
            Ok(())
        }
    }
}

pub fn build(tree: &Value, wrap: bool) -> Result<Diagram, Error> {
    let mut b = Builder {
        d: Some(Diagram { participants: vec![], messages: vec![], notes: vec![], frames: vec![], boxes: vec![], wrap }),
        ..Default::default()
    };
    b.statements(tree.get("statements"))?;
    let d = b.d.take().expect("diagram");
    if d.participants.iter().any(|p| p.id.is_empty()) {
        return Err(unported());
    }
    Ok(d)
}

const COLOR_NAMES: &str = "aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato transparent turquoise violet wheat white whitesmoke yellow yellowgreen";

/// `SequenceBoxTitle.split`: an optional leading colour, then the title.
fn box_title(text: &str) -> (Option<String>, String) {
    use regex::Regex;
    let function = Regex::new(r"(?s)\A((?:rgba?|hsla?)[ \t\r\n\x0b\x0c]*\([^)]*\))[ \t\r\n\x0b\x0c]*(.*)\z").expect("regex");
    if let Some(m) = function.captures(text) {
        return (Some(m[1].to_string()), strip(&m[2]).to_string());
    }
    let word = Regex::new(r"(?s)\A([A-Za-z0-9_]+)[ \t\r\n\x0b\x0c]*(.*)\z").expect("regex");
    if let Some(m) = word.captures(text) {
        if COLOR_NAMES.split(' ').any(|n| n == m[1].to_lowercase()) {
            return (Some(m[1].to_string()), strip(&m[2]).to_string());
        }
    }
    (None, text.to_string())
}

/// The `type` of `@{ "type": "boundary" }`, lower-cased.
fn shape_type(metadata: &str) -> Option<String> {
    let re = regex::Regex::new(r#"["']?type["']?[ \t\r\n\x0b\x0c]*:[ \t\r\n\x0b\x0c]*["']?([A-Za-z]+)"#).expect("regex");
    re.captures(metadata).map(|c| c[1].to_lowercase())
}
