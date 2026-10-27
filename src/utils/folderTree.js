// Subfolders aren't stored anywhere — they're derived from the tracks a scan
// returned: every track carries `relativeDir` (its folder, relative to the
// root it was found under, null for the root itself). So only directories
// that actually contain audio (or lead to one) show up in the tree.

// The sidebar selects either a whole top-level folder ("<id>") or one of its
// subfolders ("<id>::sub/dir") — one string, so it fits the single
// activeFolderId the rest of the app already uses.
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

export function isInSubfolder(track, sub) {
  if (!sub) return true;
  const rel = normalizeRel(track.relativeDir);
  return rel === sub || rel.startsWith(sub + "/");
}

const byName = (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true });

// -> [{ name, rel, children: [...] }] sorted by name at every level.
export function buildTree(tracks) {
  const root = { children: new Map() };
  const seen = new Set();
  for (const t of tracks) {
    const rel = normalizeRel(t.relativeDir);
    if (!rel || seen.has(rel)) continue;
    seen.add(rel);
    let node = root;
    let path = "";
    for (const part of rel.split("/")) {
      path = path ? `${path}/${part}` : part;
      if (!node.children.has(part)) node.children.set(part, { name: part, rel: path, children: new Map() });
      node = node.children.get(part);
    }
  }
  const finish = (node) =>
    [...node.children.values()]
      .map((n) => ({ name: n.name, rel: n.rel, children: finish(n) }))
      .sort(byName);
  return finish(root);
}

// Moves `keys` (keeping their relative order) to sit just before/after
// `targetKey` within `list`. Used for both the top-level folder order and
// a subfolder's custom order among its siblings.
export function moveKeys(list, keys, targetKey, pos) {
  const moving = list.filter((k) => keys.includes(k));
  const rest = list.filter((k) => !keys.includes(k));
  let to = rest.indexOf(targetKey);
  if (to === -1 || moving.length === 0) return list;
  if (pos === "after") to += 1;
  rest.splice(to, 0, ...moving);
  return rest;
}

// Sorts tree nodes by a saved custom order (a list of rel paths); anything
// not in it keeps its alphabetical position after the listed ones.
export function applyOrder(nodes, order) {
  if (!order || order.length === 0) return nodes;
  const idx = new Map(order.map((rel, i) => [rel, i]));
  const at = (n) => (idx.has(n.rel) ? idx.get(n.rel) : Infinity);
  return [...nodes].sort((a, b) => {
    const ai = at(a);
    const bi = at(b);
    return ai === bi ? 0 : ai - bi;
  });
}
