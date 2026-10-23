import { useEffect, useMemo, useRef, useState } from "react";
import { useDisc } from "../context/DiscContext.jsx";
import { buildTree, selectionKey, parseSelection } from "../utils/folderTree.js";
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
function Row({ depth, hasChildren, open, onToggle, active, selected, onSelect, name, title, className = "", children, ...rest }) {
  return (
    <div
      className={
        "sidebar__row" +
        (active ? " sidebar__row--active" : "") +
        (selected ? " sidebar__row--selected" : "") +
        (className ? " " + className : "")
      }
      style={{ paddingLeft: BASE_PAD_PX + depth * INDENT_PX }}
      role="button"
      tabIndex={0}
      onClick={(e) => onSelect(e)}
      onKeyDown={(e) => {
        if (e.key === "Enter") onSelect({});
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
    onRemoveFolders,
    onRenameFolder,
    onMoveFolder,
  } = useDisc();
  const [expanded, setExpanded] = useState(loadExpanded);
  const [editingId, setEditingId] = useState(null);
  // Multi-select of top-level folders: shift-click a range, ctrl-click to
  // toggle. Separate from activeFolderId, which only decides what the track
  // list shows.
  const [selectedIds, setSelectedIds] = useState(() => new Set());
  const anchorIdRef = useRef(null);
  const [removeTargets, setRemoveTargets] = useState(null); // folder[] | null
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

  // Forget selected folders that no longer exist (removed elsewhere).
  useEffect(() => {
    setSelectedIds((prev) => {
      const live = new Set(folders.map((f) => f.id));
      const next = new Set([...prev].filter((id) => live.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [folders]);

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

  function handleFolderClick(e, folder) {
    const ids = folders.map((f) => f.id);
    // A modifier-click with nothing selected yet starts from the folder
    // you're currently viewing, like an explorer would.
    const viewing = parseSelection(activeFolderId).folderId;
    const anchor =
      anchorIdRef.current && ids.includes(anchorIdRef.current)
        ? anchorIdRef.current
        : ids.includes(viewing)
        ? viewing
        : null;
    if (e.shiftKey && anchor) {
      const a = ids.indexOf(anchor);
      const b = ids.indexOf(folder.id);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      setSelectedIds(new Set(ids.slice(lo, hi + 1)));
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      setSelectedIds((prev) => {
        const next = new Set(prev.size === 0 && anchor ? [anchor] : prev);
        if (next.has(folder.id)) next.delete(folder.id);
        else next.add(folder.id);
        return next;
      });
      anchorIdRef.current = folder.id;
      return;
    }
    anchorIdRef.current = folder.id;
    setSelectedIds(new Set());
    setActiveFolderId(folder.id);
  }

  // What × / the Remove button / Delete act on: the whole selection if
  // there is one and it includes the folder in question, else just that one.
  function requestRemove(folder) {
    if (folder && selectedIds.size > 1 && selectedIds.has(folder.id)) {
      setRemoveTargets(folders.filter((f) => selectedIds.has(f.id)));
    } else if (folder) {
      setRemoveTargets([folder]);
    } else if (selectedIds.size > 0) {
      setRemoveTargets(folders.filter((f) => selectedIds.has(f.id)));
    }
  }

  function handleKeyDown(e) {
    if (editingId) return;
    if (e.key === "Delete" && selectedIds.size > 0) {
      e.preventDefault();
      requestRemove(null);
    } else if (e.key === "Escape" && selectedIds.size > 0) {
      setSelectedIds(new Set());
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      setSelectedIds(new Set(folders.map((f) => f.id)));
    }
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
      <div className="sidebar__scroll" onKeyDown={handleKeyDown}>
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
                  selected={selectedIds.has(folder.id)}
                  onSelect={(e) => handleFolderClick(e, folder)}
                  name={folder.name}
                  className={missing ? "sidebar__row--missing" : ""}
                  title={
                    missing
                      ? `Can't reach ${folder.folderPath} right now`
                      : `${folder.folderPath} — double-click to rename, drag to reorder, shift/ctrl-click to select several`
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
                    title={
                      selectedIds.size > 1 && selectedIds.has(folder.id)
                        ? `Remove all ${selectedIds.size} selected folders from Disc (your files aren't touched)`
                        : "Remove from Disc (your files aren't touched)"
                    }
                    onClick={(e) => {
                      e.stopPropagation();
                      requestRemove(folder);
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

      {selectedIds.size > 1 && (
        <div className="sidebar__selection">
          <span>{selectedIds.size} folders selected</span>
          <div className="sidebar__selection-actions">
            <button className="sidebar__selection-btn" onClick={() => setSelectedIds(new Set())}>
              Clear
            </button>
            <button
              className="sidebar__selection-btn sidebar__selection-btn--danger"
              onClick={() => requestRemove(null)}
              title="Remove from Disc — your files aren't touched"
            >
              Remove
            </button>
          </div>
        </div>
      )}

      {removeTargets && (
        <ConfirmModal
          title={removeTargets.length > 1 ? "Remove folders" : "Remove folder"}
          message={
            removeTargets.length > 1
              ? `Remove ${removeTargets.length} folders from Disc? Your files on disk aren't touched — they just stop showing here.`
              : `Remove "${removeTargets[0].name}" from Disc? Your files on disk aren't touched — it just stops showing here.`
          }
          confirmLabel={removeTargets.length > 1 ? `Remove ${removeTargets.length}` : "Remove"}
          onConfirm={() => {
            onRemoveFolders(removeTargets.map((f) => f.id));
            setSelectedIds(new Set());
            setRemoveTargets(null);
          }}
          onCancel={() => setRemoveTargets(null)}
        />
      )}
    </aside>
  );
}
