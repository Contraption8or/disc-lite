// The sidebar tree. There are two kinds of node:
//   - "folder": a directory you added to Disc (key = its id)
//   - "sub":    a directory inside one of those, derived from the tracks a scan
//               found (key = "<folderId>::sub/dir")
// Every node starts out under its natural parent (subfolders under the
// directory that contains them, folders at the top level), but any node can
// be moved under any other — purely inside Disc, nothing on disk changes.
// Those moves are `placement` ({ nodeKey: parentKey, "" = top level }), and
// the order of each parent's children is `order` ({ parentKey: [nodeKey,...] }).

const SEP = "::";

export function selectionKey(folderId, rel) {
  return rel ? `${folderId}${SEP}${rel}` : folderId;
}

export function parseSelection(key) {
  const i = typeof key === "string" ? key.indexOf(SEP) : -1;
  if (i === -1) return { folderId: key, sub: "" };
  return { folderId: key.slice(0, i), sub: key.slice(i + SEP.length) };
}

// Scan results use the OS separator; normalize so comparisons are stable.
export function normalizeRel(dir) {
  return dir ? dir.split(/[\\/]/).filter(Boolean).join("/") : "";
}

const byName = (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true });

// Earlier versions only let subfolders be reordered, saved as
// { parentKey: [rel, ...] } — turn that into the current { parentKey: [nodeKey, ...] }.
export function migrateSubOrder(subOrder) {
  const out = {};
  for (const [parentKey, rels] of Object.entries(subOrder || {})) {
    if (!Array.isArray(rels)) continue;
    const { folderId } = parseSelection(parentKey);
    out[parentKey] = rels.map((rel) => selectionKey(folderId, rel));
  }
  return out;
}

// Builds the whole tree: nodes, each node's effective children (in display
// order), and each node's own tracks.
//   -> { nodes: Map<key, node>, childrenOf: Map<parentKey, node[]>,
//        effParent: Map<key, parentKey>, direct: Map<key, track[]> }
export function buildModel(folders, folderTracks, placement, order) {
  const nodes = new Map();
  const direct = new Map();

  folders.forEach((f, i) => {
    nodes.set(f.id, { key: f.id, kind: "folder", folderId: f.id, rel: "", name: f.name, naturalParent: "", idx: i });
    const byRel = new Map();
    for (const t of folderTracks[f.id] || []) {
      const rel = normalizeRel(t.relativeDir);
      if (!byRel.has(rel)) byRel.set(rel, []);
      byRel.get(rel).push(t);
    }
    direct.set(f.id, byRel.get("") || []);
    for (const [rel, tracks] of byRel) {
      if (!rel) continue;
      direct.set(selectionKey(f.id, rel), tracks);
      // The directory itself plus every ancestor directory leading to it.
      let path = "";
      for (const part of rel.split("/")) {
        const parentPath = path;
        path = path ? `${path}/${part}` : part;
        const key = selectionKey(f.id, path);
        if (!nodes.has(key)) {
          nodes.set(key, {
            key,
            kind: "sub",
            folderId: f.id,
            rel: path,
            name: part,
            naturalParent: parentPath ? selectionKey(f.id, parentPath) : f.id,
            idx: 0,
          });
        }
      }
    }
  });

  // Effective parent: a saved move, if it still points at something real,
  // otherwise the natural one.
  const effParent = new Map();
  for (const n of nodes.values()) {
    const moved = placement?.[n.key];
    const valid = moved !== undefined && moved !== n.key && (moved === "" || nodes.has(moved));
    effParent.set(n.key, valid ? moved : n.naturalParent);
  }
  // A cycle (A under B under A) can't be shown — put such nodes back where
  // they naturally belong.
  for (const n of nodes.values()) {
    const seen = new Set();
    let cur = n.key;
    while (cur !== "" && cur !== undefined && !seen.has(cur)) {
      seen.add(cur);
      cur = effParent.get(cur);
    }
    if (cur !== "" && cur !== undefined) effParent.set(n.key, n.naturalParent);
  }

  const childrenOf = new Map([["", []]]);
  for (const n of nodes.values()) {
    const p = effParent.get(n.key);
    if (!childrenOf.has(p)) childrenOf.set(p, []);
    childrenOf.get(p).push(n);
  }
  for (const [parentKey, list] of childrenOf) {
    const saved = order?.[parentKey];
    const idx = new Map((Array.isArray(saved) ? saved : []).map((k, i) => [k, i]));
    const at = (n) => (idx.has(n.key) ? idx.get(n.key) : Infinity);
    list.sort((a, b) => {
      const ai = at(a);
      const bi = at(b);
      if (ai !== bi) return ai - bi;
      // Unlisted nodes: the top level follows the order folders were added
      // in; everywhere else it's alphabetical.
      if (parentKey === "" && a.kind === "folder" && b.kind === "folder") return a.idx - b.idx;
      return byName(a, b);
    });
  }

  return { nodes, childrenOf, effParent, direct };
}

// Is `candidateKey` the same as, or somewhere underneath, `ancestorKey`?
export function isWithin(model, candidateKey, ancestorKey) {
  let cur = candidateKey;
  const seen = new Set();
  while (cur !== "" && cur !== undefined && !seen.has(cur)) {
    if (cur === ancestorKey) return true;
    seen.add(cur);
    cur = model.effParent.get(cur);
  }
  return false;
}

// What a node shows in the track list: its own tracks plus everything
// nested under it (in the order the tree shows them).
export function nodeTracks(model, key) {
  const out = [];
  const seen = new Set();
  const walk = (k) => {
    const node = model.nodes.get(k);
    if (!node) return;
    for (const t of model.direct.get(k) || []) {
      if (seen.has(t.id)) continue;
      seen.add(t.id);
      out.push({ ...t, folderId: node.folderId });
    }
    for (const child of model.childrenOf.get(k) || []) walk(child.key);
  };
  walk(key);
  return out;
}
