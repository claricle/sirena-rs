//! Stage 3: detect the diagram type and parse its body with the PARG artifact
//! (`artifact/mermaid.json`, built by claricle/etassia-mermaid-parg) through the
//! parsanol engine. The tree equals the Ruby native tree
//! (`Etassia::Parser::Native.parse`) for every type.

mod detect;
mod source;

use crate::Error;
use parsanol::PargArtifact;
use std::sync::OnceLock;

/// The parse tree: parsanol-tree/v2 as JSON. Terminals are slice objects
/// `{value, line, column, offset, length}`; offsets are bytes.
pub type Tree = serde_json::Value;

const ARTIFACT: &str = include_str!("../../artifact/mermaid.json");

fn artifact() -> &'static PargArtifact {
    static CELL: OnceLock<PargArtifact> = OnceLock::new();
    CELL.get_or_init(|| PargArtifact::from_json(ARTIFACT).expect("embedded mermaid artifact loads"))
}

/// Diagram type name for `source` (same names as the corpus directories,
/// e.g. "pie", "state_diagram"), or None when no type matches.
pub fn detect(source: &str) -> Option<String> {
    detect::detect_body(&source::body(source)).map(str::to_string)
}

/// The diagram body of `source`: preamble (frontmatter, directives, comments) removed.
pub fn body(source: &str) -> String {
    source::body(source)
}

/// Parses the body of `source` with the grammar entry for `diagram_type`.
pub fn parse(source: &str, diagram_type: &str) -> Result<Tree, Error> {
    let artifact = artifact();
    if !artifact.entry_names().contains(&diagram_type) {
        return Err(Error::Unported(diagram_type.to_string()));
    }
    let body = source::body(source);
    artifact.parse_shape(diagram_type, &body).map_err(|e| match e {
        parsanol::PargError::ParseWire { line, column, .. } => Error::Parse {
            class: "ParseError".into(),
            message: e.to_string(),
            line,
            col: column,
        },
        other => Error::Parse {
            class: "ParseError".into(),
            message: other.to_string(),
            line: 0,
            col: 0,
        },
    })
}
