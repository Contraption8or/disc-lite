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
