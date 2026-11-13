//! Port of `Etassia::Notation::Mermaid::TYPES` patterns, in detection order.
//! Names are the corpus directory names.

use regex::Regex;
use std::sync::OnceLock;

const TYPES: &[(&str, &str)] = &[
    ("flowchart", r"\A(?-u:\s)*(?:flowchart-elk|(?i:graph|flowchart))(?:[ \t\n\r\x0b\x0c<>^]|\z)"),
    ("sequence", r"(?i)\A(?-u:\s)*sequenceDiagram"),
    ("class_diagram", r"(?i)\A(?-u:\s)*classDiagram"),
    ("state_diagram", r"(?i)\A(?-u:\s)*stateDiagram"),
    ("er_diagram", r"(?i)\A(?-u:\s)*erDiagram"),
    ("user_journey", r"(?i)\A(?-u:\s)*journey"),
    ("gantt", r"(?i)\A(?-u:\s)*gantt(?:(?-u:\s)|\z)"),
    ("pie", r"(?i)\A(?-u:\s)*pie(?:(?-u:\s)|\z)"),
    ("timeline", r"(?i)\A(?-u:\s)*timeline(?:(?-u:\s)|(?m:$))"),
    ("quadrant", r"(?i)\A(?-u:\s)*quadrantChart"),
    ("gitgraph", r"(?i)\A(?-u:\s)*gitGraph"),
    ("mindmap", r"(?i)\A(?-u:\s)*mindmap"),
    ("kanban", r"(?i)\A(?-u:\s)*kanban"),
    ("radar", r"(?i)\A(?-u:\s)*radar-beta"),
    ("block", r"(?i)\A(?-u:\s)*block-beta"),
    ("requirement", r"(?i)\A(?-u:\s)*requirementDiagram"),
    ("xychart", r"(?i)\A(?-u:\s)*xychart-beta"),
    ("architecture", r"(?i)\A(?-u:\s)*architecture-beta"),
    ("sankey", r"(?i)\A(?-u:\s)*sankey-beta"),
    ("packet", r"(?i)\A(?-u:\s)*packet-beta"),
    ("treemap", r"(?i)\A(?-u:\s)*treemap"),
    (
        "c4",
        r"(?i)\A(?-u:\s)*(?:C4Context|C4Container|C4Component|C4Dynamic|C4Deployment|C4(?-u:\s)+diagram)",
    ),
    ("info", r"(?i)\A(?-u:\s)*info"),
    ("error", r"(?i)\A(?-u:\s)*(?:error)"),
];

fn table() -> &'static Vec<(&'static str, Regex)> {
    static CELL: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    CELL.get_or_init(|| {
        TYPES
            .iter()
            .map(|(name, pattern)| (*name, Regex::new(pattern).expect("detect pattern")))
            .collect()
    })
}

/// First type whose pattern matches the (preamble-free) body.
pub fn detect_body(body: &str) -> Option<&'static str> {
    table().iter().find(|(_, re)| re.is_match(body)).map(|(name, _)| *name)
}
