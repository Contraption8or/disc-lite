import { useEffect, useMemo, useRef, useState } from "react";
import { useDisc } from "../context/DiscContext.jsx";
import { buildTree, selectionKey } from "../utils/folderTree.js";
import ConfirmModal from "./ConfirmModal.jsx";
import Icon from "./Icon.jsx";
import "./Sidebar.css";

const EXPANDED_KEY = "lite.expanded";
const INDENT_PX = 14;
const BASE_PAD_PX = 6;

function loadExpanded() {
  try {
    const parsed = JSON.parse(localStorage.getItem(EXPANDED_KEY) || "[]");
    return new Set(Array.isArray(parsed) ? parsed : []);
  } catch {
    return new Set();
  }
}

// One tree row: [chevron] name. The chevron slot is always reserved (just
// hidden when there's nothing to expand) so every name lines up.
function Row({ depth, hasChildren, open, onToggle, active, onSelect, name, title, className = "", children, ...rest }) {
  return (
    <div
      className={"sidebar__row" + (active ? " sidebar__row--active" : "") + (className ? " " + className : "")}
      style={{ paddingLeft: BASE_PAD_PX + depth * INDENT_PX }}
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter") onSelect();
      }}
      title={title}
      {...rest}
    >
      <span
        className={"sidebar__chevron" + (open ? " sidebar__chevron--open" : "")}
        style={{ visibility: hasChildren ? "visible" : "hidden" }}
        onClick={(e) => {
          e.stopPropagation();
          onToggle?.();
        }}
      >
        <Icon name="chevronRight" size={11} />
      </span>
      <span className="sidebar__name">{name}</span>
      {children}
    </div>
  );
}

// The one left-hand column. Its width is fixed (--sidebar-width) — it never
// grows or shrinks with the window; the track list takes whatever's left.
// Folders are a VS Code–style tree: expand one to see the subfolders inside
// it, and drag top-level folders to reorder them.
export default function Sidebar() {
  const {
    folders,
    folderTracks,
    activeFolderId,
    setActiveFolderId,
    missingFolderIds,
    onAddFolders,
    onRemoveFolder,
    onRenameFolder,
    onMoveFolder,
  } = useDisc();
  const [expanded, setExpanded] = useState(loadExpanded);
  const [editingId, setEditingId] = useState(null);
  const [removeTarget, setRemoveTarget] = useState(null); // folder | null
  const [dragId, setDragId] = useState(null);
  const [dropHint, setDropHint] = useState(null); // { id, pos: "before" | "after" } | null
  const inputRef = useRef(null);

  useEffect(() => {
    localStorage.setItem(EXPANDED_KEY, JSON.stringify([...expanded]));
  }, [expanded]);

  useEffect(() => {
    if (editingId) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editingId]);

  const trees = useMemo(() => {
    const out = {};
    for (const f of folders) out[f.id] = buildTree(folderTracks[f.id] || []);
    return out;
  }, [folders, folderTracks]);

  function toggle(key) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function commitRename(id, value) {
    setEditingId(null);
    onRenameFolder(id, value);
  }

  function renderSubfolders(folderId, nodes, depth) {
    return nodes.map((node) => {
      const key = selectionKey(folderId, node.rel);
      const open = expanded.has(key);
      const hasChildren = node.children.length > 0;
      return (
        <div key={key}>
          <Row
            depth={depth}
            hasChildren={hasChildren}
            open={open}
            onToggle={() => toggle(key)}
            active={activeFolderId === key}
            onSelect={() => setActiveFolderId(key)}
            name={node.name}
            title={node.rel}
          />
          {open && hasChildren && (
            <div
              className="sidebar__children"
              style={{ "--guide-x": `${BASE_PAD_PX + depth * INDENT_PX + 8}px` }}
            >
              {renderSubfolders(folderId, node.children, depth + 1)}
            </div>
          )}
        </div>
      );
    });
  }

  function handleDragOver(e, folder) {
    if (!dragId) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    if (folder.id === dragId) {
      setDropHint(null);
      return;
    }
    const rect = e.currentTarget.getBoundingClientRect();
    const pos = e.clientY < rect.top + rect.height / 2 ? "before" : "after";
    setDropHint((prev) => (prev?.id === folder.id && prev.pos === pos ? prev : { id: folder.id, pos }));
  }

  function handleDrop(e, folder) {
    if (!dragId) return;
    e.preventDefault();
    if (dropHint && dragId !== folder.id) onMoveFolder(dragId, folder.id, dropHint.pos);
    setDragId(null);
    setDropHint(null);
  }

  return (
    <aside className="sidebar">
      <div className="sidebar__scroll">
        <Row
          depth={0}
          hasChildren={false}
          active={activeFolderId === "all"}
          onSelect={() => setActiveFolderId("all")}
          name="All Music"
        />
        <Row
          depth={0}
          hasChildren={false}
          active={activeFolderId === "favorites"}
          onSelect={() => setActiveFolderId("favorites")}
          name="Favorites"
        />

        <div className="sidebar__heading">
          <span>Folders</span>
          <button className="sidebar__add" onClick={onAddFolders} title="Add folders">
            <Icon name="newGroup" size={13} />
          </button>
        </div>

        {folders.length === 0 && (
          <button className="sidebar__empty" onClick={onAddFolders}>
            + Add your music folders
          </button>
        )}

        {folders.map((folder) => {
          const missing = missingFolderIds.has(folder.id);
          const nodes = trees[folder.id] || [];
          const hasChildren = nodes.length > 0;
          const open = expanded.has(folder.id);
          const hint = dropHint?.id === folder.id ? dropHint.pos : null;
          return (
            <div
              key={folder.id}
              className={
                "sidebar__block" +
                (hint ? ` sidebar__block--drop-${hint}` : "") +
                (dragId === folder.id ? " sidebar__block--dragging" : "")
              }
              onDragOver={(e) => handleDragOver(e, folder)}
              onDrop={(e) => handleDrop(e, folder)}
            >
              {editingId === folder.id ? (
                <div className="sidebar__row" style={{ paddingLeft: BASE_PAD_PX + 16 }}>
                  <input
                    ref={inputRef}
                    className="sidebar__rename"
                    defaultValue={folder.name}
                    onBlur={(e) => commitRename(folder.id, e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") e.currentTarget.blur();
                      if (e.key === "Escape") setEditingId(null);
                    }}
                  />
                </div>
              ) : (
                <Row
                  depth={0}
                  hasChildren={hasChildren}
                  open={open}
                  onToggle={() => toggle(folder.id)}
                  active={activeFolderId === folder.id}
                  onSelect={() => setActiveFolderId(folder.id)}
                  name={folder.name}
                  className={missing ? "sidebar__row--missing" : ""}
                  title={
                    missing
                      ? `Can't reach ${folder.folderPath} right now`
                      : `${folder.folderPath} — double-click to rename, drag to reorder`
                  }
                  draggable
                  onDoubleClick={() => setEditingId(folder.id)}
                  onDragStart={(e) => {
                    e.dataTransfer.effectAllowed = "move";
                    e.dataTransfer.setData("text/plain", folder.id);
                    setDragId(folder.id);
                  }}
                  onDragEnd={() => {
                    setDragId(null);
                    setDropHint(null);
                  }}
                >
                  <button
                    className="sidebar__remove"
                    title="Remove from Disc (your files aren't touched)"
                    onClick={(e) => {
                      e.stopPropagation();
                      setRemoveTarget(folder);
                    }}
                  >
                    ×
                  </button>
                </Row>
              )}
              {open && hasChildren && (
                <div className="sidebar__children" style={{ "--guide-x": `${BASE_PAD_PX + 8}px` }}>
                  {renderSubfolders(folder.id, nodes, 1)}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {removeTarget && (
        <ConfirmModal
          title="Remove folder"
          message={`Remove "${removeTarget.name}" from Disc? Your files on disk aren't touched — it just stops showing here.`}
          confirmLabel="Remove"
          onConfirm={() => {
            onRemoveFolder(removeTarget.id);
            setRemoveTarget(null);
          }}
          onCancel={() => setRemoveTarget(null)}
        />
      )}
    </aside>
  );
}
