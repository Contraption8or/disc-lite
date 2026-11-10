//! The sidebar tree. Two kinds of node:
//!   - Folder: a directory the user added (key = its id)
//!   - Sub:    a directory inside one of those, derived from the tracks a scan
//!             found (key = "<folderId>::sub/dir")
//! Every node starts under its natural parent, but any node can be moved
//! under any other — purely inside Disc, nothing on disk changes. Moves are
//! `placement` ({ nodeKey: parentKey }, "" = top level) and each parent's
//! child order is `order` ({ parentKey: [nodeKey, ...] }). Same keys and
//! formats as the Electron version, so an imported arrangement just works.

use std::collections::{HashMap, HashSet};

use crate::model::{Folder, TrackRef};

const SEP: &str = "::";

pub fn selection_key(folder_id: &str, rel: &str) -> String {
    if rel.is_empty() {
        folder_id.to_string()
    } else {
        format!("{folder_id}{SEP}{rel}")
    }
}

/// -> (folder id, sub path) — sub is "" for a whole folder.
pub fn parse_selection(key: &str) -> (&str, &str) {
    match key.find(SEP) {
        Some(i) => (&key[..i], &key[i + SEP.len()..]),
        None => (key, ""),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Folder,
    Sub,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub key: String,
    pub kind: NodeKind,
    pub folder_id: String,
    #[allow(dead_code)]
    pub rel: String,
    pub name: String,
    pub natural_parent: String,
    /// Position among added folders (top-level default order).
    pub idx: usize,
}

#[derive(Default)]
pub struct Model {
    pub nodes: HashMap<String, Node>,
    /// parent key ("" = top level) -> child keys, in display order.
    pub children_of: HashMap<String, Vec<String>>,
    pub eff_parent: HashMap<String, String>,
    /// node key -> the tracks sitting directly in that directory.
    pub direct: HashMap<String, Vec<TrackRef>>,
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    // Case-insensitive, with digit runs compared numerically ("Track 2" < "Track 10").
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    bi.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub fn build_model(
    folders: &[Folder],
    folder_tracks: &HashMap<String, Vec<TrackRef>>,
    placement: &HashMap<String, String>,
    order: &HashMap<String, Vec<String>>,
) -> Model {
    let mut m = Model::default();

    for (i, f) in folders.iter().enumerate() {
        m.nodes.insert(
            f.id.clone(),
            Node {
                key: f.id.clone(),
                kind: NodeKind::Folder,
                folder_id: f.id.clone(),
                rel: String::new(),
                name: f.name.clone(),
                natural_parent: String::new(),
                idx: i,
            },
        );
        let mut by_rel: HashMap<&str, Vec<TrackRef>> = HashMap::new();
        if let Some(tracks) = folder_tracks.get(&f.id) {
            for t in tracks {
                by_rel.entry(t.rel_dir.as_str()).or_default().push(t.clone());
            }
        }
        for (rel, tracks) in by_rel {
            m.direct.insert(selection_key(&f.id, rel), tracks);
            if rel.is_empty() {
                continue;
            }
            // The directory itself plus every ancestor leading to it.
            let mut path = String::new();
            for part in rel.split('/') {
                let parent_path = path.clone();
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(part);
                let key = selection_key(&f.id, &path);
                m.nodes.entry(key.clone()).or_insert_with(|| Node {
                    key,
                    kind: NodeKind::Sub,
                    folder_id: f.id.clone(),
                    rel: path.clone(),
                    name: part.to_string(),
                    natural_parent: if parent_path.is_empty() {
                        f.id.clone()
                    } else {
                        selection_key(&f.id, &parent_path)
                    },
                    idx: 0,
                });
            }
        }
    }

    // Effective parent: a saved move if it still points at something real,
    // otherwise the natural one.
    for n in m.nodes.values() {
        let moved = placement.get(&n.key);
        let valid = moved.is_some_and(|p| p != &n.key && (p.is_empty() || m.nodes.contains_key(p)));
        let parent = if valid { moved.unwrap().clone() } else { n.natural_parent.clone() };
        m.eff_parent.insert(n.key.clone(), parent);
    }
    // A cycle (A under B under A) can't be shown — put such nodes back where
    // they naturally belong.
    let keys: Vec<String> = m.nodes.keys().cloned().collect();
    for key in &keys {
        let mut seen = HashSet::new();
        let mut cur = key.clone();
        while !cur.is_empty() && seen.insert(cur.clone()) {
            match m.eff_parent.get(&cur) {
                Some(p) => cur = p.clone(),
                None => break,
            }
        }
        if !cur.is_empty() {
            let natural = m.nodes[key].natural_parent.clone();
            m.eff_parent.insert(key.clone(), natural);
        }
    }

    m.children_of.insert(String::new(), Vec::new());
    for n in m.nodes.values() {
        let p = m.eff_parent[&n.key].clone();
        m.children_of.entry(p).or_default().push(n.key.clone());
    }
    let nodes = &m.nodes;
    for (parent, list) in m.children_of.iter_mut() {
        let saved: HashMap<&str, usize> = order
            .get(parent)
            .map(|v| v.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect())
            .unwrap_or_default();
        list.sort_by(|a, b| {
            let (na, nb) = (&nodes[a], &nodes[b]);
            let ai = saved.get(a.as_str()).copied().unwrap_or(usize::MAX);
            let bi = saved.get(b.as_str()).copied().unwrap_or(usize::MAX);
            ai.cmp(&bi).then_with(|| {
                // Unlisted: the top level keeps the order folders were added
                // in; everywhere else is alphabetical.
                if parent.is_empty() && na.kind == NodeKind::Folder && nb.kind == NodeKind::Folder {
                    na.idx.cmp(&nb.idx)
                } else {
                    natural_cmp(&na.name, &nb.name)
                }
            })
        });
    }
    m
}

impl Model {
    pub fn children(&self, key: &str) -> &[String] {
        self.children_of.get(key).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Is `candidate` the same as, or somewhere underneath, `ancestor`?
    pub fn is_within(&self, candidate: &str, ancestor: &str) -> bool {
        let mut cur = candidate.to_string();
        let mut seen = HashSet::new();
        while !cur.is_empty() && seen.insert(cur.clone()) {
            if cur == ancestor {
                return true;
            }
            match self.eff_parent.get(&cur) {
                Some(p) => cur = p.clone(),
                None => return false,
            }
        }
        false
    }

    /// What a node shows in the track list: its own tracks plus everything
    /// nested under it, in tree order.
    pub fn node_tracks(&self, key: &str) -> Vec<TrackRef> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.walk_tracks(key, &mut out, &mut seen);
        out
    }

    fn walk_tracks(&self, key: &str, out: &mut Vec<TrackRef>, seen: &mut HashSet<String>) {
        if !self.nodes.contains_key(key) {
            return;
        }
        if let Some(ts) = self.direct.get(key) {
            for t in ts {
                if seen.insert(t.id.clone()) {
                    out.push(t.clone());
                }
            }
        }
        for c in self.children(key) {
            self.walk_tracks(c, out, seen);
        }
    }
}

/// Moves `keys` under `parent_key` ("" = top level) just before `before_key`
/// (or at the end). Updates `order` and `placement` in place. Returns false
/// if the move isn't allowed (into itself or one of its own descendants).
pub fn move_nodes(
    model: &Model,
    placement: &mut HashMap<String, String>,
    order: &mut HashMap<String, Vec<String>>,
    keys: &[String],
    parent_key: &str,
    before_key: Option<&str>,
) -> bool {
    if keys.is_empty() || !keys.iter().all(|k| model.nodes.contains_key(k)) {
        return false;
    }
    if !parent_key.is_empty() {
        if !model.nodes.contains_key(parent_key) {
            return false;
        }
        if keys.iter().any(|k| model.is_within(parent_key, k)) {
            return false;
        }
    }
    let siblings: Vec<String> = model.children(parent_key).to_vec();
    // If the spot we were asked to drop before is itself moving, use the
    // next sibling that isn't.
    let mut anchor = before_key.map(|s| s.to_string());
    if let Some(a) = anchor.clone() {
        if keys.contains(&a) {
            let start = siblings.iter().position(|k| *k == a).unwrap_or(0);
            anchor = siblings[start + 1..].iter().find(|k| !keys.contains(k)).cloned();
        }
    }
    let mut rest: Vec<String> = siblings.into_iter().filter(|k| !keys.contains(k)).collect();
    let at = anchor
        .and_then(|a| rest.iter().position(|k| *k == a))
        .unwrap_or(rest.len());
    for (i, k) in keys.iter().enumerate() {
        rest.insert(at + i, k.clone());
    }
    order.insert(parent_key.to_string(), rest);
    for k in keys {
        if model.nodes[k].natural_parent == parent_key {
            placement.remove(k);
        } else {
            placement.insert(k.clone(), parent_key.to_string());
        }
    }
    true
}
