//! Stage 4 dispatch. Owner: integration. Each type module is owned by its builder.
pub mod info;
pub mod pie;
pub mod packet;
pub mod sankey;
pub mod timeline;
pub mod treemap;
pub mod quadrant;
pub mod radar;
pub mod xychart;
pub mod user_journey;
pub mod kanban;
pub mod mindmap;
pub mod gantt;
pub mod gitgraph;
pub mod er_diagram;
pub mod requirement;
pub mod c4;
pub mod block;
pub mod architecture;
pub mod sequence;
pub mod state_diagram;
pub mod class_diagram;
pub mod flowchart;
use crate::Error;
/// Render `source` to SVG, or Err(Unported) when the type has no Rust renderer.
pub fn render(source: &str) -> Result<String, Error> {
    let ty = crate::detect(source).ok_or_else(|| Error::Unported("unknown".into()))?;
    match ty.as_str() {
        "info" => info::render(source),
        "pie" => pie::render(source),
        "packet" => packet::render(source),
        "sankey" => sankey::render(source),
        "timeline" => timeline::render(source),
        "treemap" => treemap::render(source),
        "quadrant" => quadrant::render(source),
        "radar" => radar::render(source),
        "xychart" => xychart::render(source),
        "user_journey" => user_journey::render(source),
        "kanban" => kanban::render(source),
        "mindmap" => mindmap::render(source),
        "gantt" => gantt::render(source),
        "gitgraph" => gitgraph::render(source),
        "er_diagram" => er_diagram::render(source),
        "requirement" => requirement::render(source),
        "c4" => c4::render(source),
        "block" => block::render(source),
        "architecture" => architecture::render(source),
        "sequence" => sequence::render(source),
        "state_diagram" => state_diagram::render(source),
        "class_diagram" => class_diagram::render(source),
        "flowchart" => flowchart::render(source),
        other => Err(Error::Unported(other.to_string())),
    }
}
