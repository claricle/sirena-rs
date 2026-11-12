//! Subgraph claiming and promotion: Ruby `Builders::Flowchart` (`claim`,
//! `record_subgraph`, `promote_referenced_subgraphs`) and the id and
//! parentage rules of `IRAdapters::Flowchart`.
//!
//! Declined (`Error::Unported`): quoted subgraph ids, titles with markup,
//! a box that holds itself, two drawn boxes with one id.
use super::*;
use std::collections::HashSet;

struct Rec {
    id: String,
    label: Option<String>,
    parent: Option<String>,
    dir: Option<&'static str>,
    node_ids: Vec<String>,
    child_ids: Vec<String>,
}

impl Rec {
    fn drawable(&self) -> bool {
        !self.node_ids.is_empty() || !self.child_ids.is_empty()
    }
}

pub(super) struct Header {
    id: Option<String>,
    label: Option<String>,
}

#[derive(Default)]
pub(super) struct Boxes {
    list: Vec<Rec>,
    first: HashMap<String, usize>,
    firsts: Vec<String>,
    claimed: HashSet<String>,
    ownership: Vec<(String, String)>,
    stack: Vec<String>,
}

fn id_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) && !RESERVED.contains(&s)
}

fn title_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | ',' | '?' | '!' | ':' | '/'))
}

/// `subgraph` arguments: `id`, `id[title]`, `id["title"]` or a free title.
pub(super) fn header(args: &str) -> Result<Header, Error> {
    let args = args.trim();
    if let Some(at) = args.find('[') {
        let (id, tail) = args.split_at(at);
        let inner = tail[1..].strip_suffix(']').ok_or_else(|| unported("subgraph title"))?.trim();
        let inner = if inner.len() >= 2 && inner.starts_with('"') && inner.ends_with('"') { &inner[1..inner.len() - 1] } else { inner };
        if !id_ok(id) || !title_ok(inner) {
            return Err(unported("subgraph title"));
        }
        return Ok(Header { id: Some(id.to_string()), label: Some(inner.trim().to_string()) });
    }
    if args.contains([' ', '\t']) {
        if !title_ok(args) {
            return Err(unported("subgraph title"));
        }
        return Ok(Header { id: None, label: Some(args.to_string()) });
    }
    if !id_ok(args) {
        return Err(unported("subgraph id"));
    }
    Ok(Header { id: Some(args.to_string()), label: None })
}

/// Generated ids (`subGraph<n>`), numbered in the order the boxes close:
/// mermaid numbers the innermost box first. Keyed by statement position.
pub(super) fn generated_ids(stmts: &[&str]) -> Result<HashMap<usize, String>, Error> {
    let (mut stack, mut out, mut n) = (vec![], HashMap::new(), 0);
    for (i, s) in stmts.iter().enumerate() {
        let s = s.trim_matches([' ', '\t']);
        if let Some(args) = keyword_args(s, "subgraph") {
            stack.push((i, header(args)?.id.is_none()));
        } else if s == "end" {
            let (at, generated) = stack.pop().ok_or_else(|| unported("end without subgraph"))?;
            if generated {
                out.insert(at, format!("subGraph{n}"));
                n += 1;
            }
        }
    }
    if !stack.is_empty() {
        return Err(unported("unclosed subgraph"));
    }
    Ok(out)
}

impl Boxes {
    pub(super) fn any(&self) -> bool {
        !self.list.is_empty()
    }

    /// `record_subgraph`: the box is built; it is a child only if its id was unclaimed.
    pub(super) fn open(&mut self, hdr: Header, generated: Option<String>) -> Result<(), Error> {
        let id = hdr.id.or(generated).ok_or_else(|| unported("subgraph id"))?;
        let parent = self.stack.last().cloned();
        let at = self.list.len();
        self.list.push(Rec { id: id.clone(), label: hdr.label, parent: None, dir: None, node_ids: vec![], child_ids: vec![] });
        let holding = *self.first.entry(id.clone()).or_insert_with(|| {
            self.firsts.push(id.clone());
            at
        });
        if let Some(holder) = self.claim(parent.as_deref(), &id) {
            self.list[holding].parent = parent;
            self.list[holder].child_ids.push(id.clone());
        }
        self.stack.push(id);
        Ok(())
    }

    pub(super) fn close(&mut self) {
        self.stack.pop();
    }

    /// The first box to name an id keeps it; a top-level statement claims nothing.
    fn claim(&mut self, parent: Option<&str>, id: &str) -> Option<usize> {
        let holder = *self.first.get(parent?)?;
        if id.is_empty() || self.claimed.contains(id) {
            return None;
        }
        self.claimed.insert(id.to_string());
        self.ownership.push((self.list[holder].id.clone(), id.to_string()));
        Some(holder)
    }

    pub(super) fn claim_member(&mut self, id: &str) {
        let parent = self.stack.last().cloned();
        if let Some(holder) = self.claim(parent.as_deref(), id) {
            self.list[holder].node_ids.push(id.to_string());
        }
    }

    pub(super) fn set_direction(&mut self, dir: &'static str) {
        if let Some(&b) = self.stack.last().and_then(|p| self.first.get(p)) {
            self.list[b].dir = Some(dir);
        }
    }

    fn cyclic(&self) -> bool {
        let mut kids: HashMap<&str, Vec<&str>> = HashMap::new();
        for (p, c) in &self.ownership {
            kids.entry(p).or_default().push(c);
        }
        for root in kids.keys() {
            let (mut pending, mut seen) = (vec![*root], HashSet::new());
            while let Some(n) = pending.pop() {
                for &c in kids.get(n).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if c == *root {
                        return true;
                    }
                    if seen.insert(c) {
                        pending.push(c);
                    }
                }
            }
        }
        false
    }

    /// Promote referenced boxes, drop the nodes they replace, and build the
    /// cluster boxes, their parentage and the node a box shares an id with.
    pub(super) fn finish(mut self, model: &mut Model) -> Result<(), Error> {
        if self.list.is_empty() {
            return Ok(());
        }
        if self.cyclic() {
            return Err(unported("subgraph cycle"));
        }
        let surviving: Vec<String> = self.firsts.iter().filter(|id| self.list[self.first[*id]].drawable()).cloned().collect();
        let mut holders: HashMap<String, usize> = HashMap::new();
        for (bi, r) in self.list.iter().enumerate() {
            for id in &r.node_ids {
                holders.insert(id.clone(), bi);
            }
        }
        for id in &surviving {
            if let Some(&h) = holders.get(id) {
                if self.list[h].id == *id {
                    return Err(unported("subgraph holds itself"));
                }
                self.list[h].node_ids.retain(|n| n != id);
                self.list[h].child_ids.push(id.clone());
                let bi = self.first[id];
                self.list[bi].parent = Some(self.list[h].id.clone());
            }
        }
        let promoted: HashSet<&str> = surviving.iter().map(|s| s.as_str()).collect();
        let drawn: Vec<usize> = (0..self.list.len()).filter(|&i| self.list[i].drawable()).collect();
        let mut seen = HashSet::new();
        if !drawn.iter().all(|&i| seen.insert(self.list[i].id.clone())) {
            return Err(unported("repeated subgraph id"));
        }
        // nodes: drop the promoted ones
        let old_ids: Vec<String> = model.nodes.iter().map(|n| n.id.clone()).collect();
        let mut remap: Vec<Option<usize>> = vec![];
        let mut kept = vec![];
        for n in std::mem::take(&mut model.nodes) {
            if promoted.contains(n.id.as_str()) {
                remap.push(None);
            } else {
                remap.push(Some(kept.len()));
                kept.push(n);
            }
        }
        model.nodes = kept;
        let colliding: HashSet<&str> = self.list.iter().filter(|r| !r.drawable()).map(|r| r.id.as_str()).collect();
        for n in &mut model.nodes {
            n.collision = colliding.contains(n.id.as_str());
        }
        // cluster boxes, in declaration order
        let mut sub_of: HashMap<String, usize> = HashMap::new();
        for (j, &bi) in drawn.iter().enumerate() {
            let r = &self.list[bi];
            let title = r.label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| r.id.clone());
            let m = text::measure(&title, FONT_SIZE, Face::Trebuchet);
            sub_of.insert(r.id.clone(), j);
            model.subs.push(Sub { node: model.nodes.len(), parent: None, dir: r.dir, members: vec![], label: (m.width, m.height) });
            model.nodes.push(Node { id: r.id.clone(), lines: vec![title], icons: vec![0], shape: Shape::Rect, fill: None, stroke: None, x: 0.0, y: 0.0, w: 0.0, h: 0.0, cluster: true, collision: false });
        }
        for (j, &bi) in drawn.iter().enumerate() {
            model.subs[j].parent = self.list[bi].parent.as_ref().and_then(|p| sub_of.get(p)).copied();
        }
        let mut holder_of: HashMap<&str, usize> = HashMap::new();
        for (j, &bi) in drawn.iter().enumerate() {
            for id in &self.list[bi].node_ids {
                holder_of.insert(id, j);
            }
        }
        for i in 0..model.nodes.len() {
            if model.nodes[i].cluster {
                continue;
            }
            if let Some(&j) = holder_of.get(model.nodes[i].id.as_str()) {
                model.subs[j].members.push(i);
            }
        }
        // edges end on a box when the id names one
        let by_id: HashMap<&str, usize> = model.nodes.iter().enumerate().filter(|(_, n)| !n.cluster).map(|(i, n)| (n.id.as_str(), i)).collect();
        let end = |old: usize| -> Option<usize> {
            let id = old_ids[old].as_str();
            match sub_of.get(id) {
                Some(&j) => Some(model.subs[j].node),
                None => remap[old].and(by_id.get(id).copied()),
            }
        };
        let mut ends = vec![];
        for e in &model.edges {
            ends.push((end(e.src).ok_or_else(|| unported("edge end"))?, end(e.tgt).ok_or_else(|| unported("edge end"))?));
        }
        for (e, (s, t)) in model.edges.iter_mut().zip(ends) {
            e.src = s;
            e.tgt = t;
        }
        Ok(())
    }
}
