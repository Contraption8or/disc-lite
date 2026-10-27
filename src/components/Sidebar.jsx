import { useEffect, useMemo, useRef, useState } from "react";
import { useDisc } from "../context/DiscContext.jsx";
import { buildTree, selectionKey, parseSelection, moveKeys, applyOrder } from "../utils/folderTree.js";
import ConfirmModal from "./ConfirmModal.jsx";
import Icon from "./Icon.jsx";
import "./Sidebar.css";

const EXPANDED_KEY = "lite.expanded";
const SUB_ORDER_KEY = "lite.subOrder";
const INDENT_PX = 14;
const BASE_PAD_PX = 6;
const DRAG_THRESHOLD_PX = 5;
const AUTOSCROLL_EDGE_PX = 40;

function loadExpanded() {
  try {
    const parsed = JSON.parse(localStorage.getItem(EXPANDED_KEY) || "[]");
    return new Set(Array.isArray(parsed) ? parsed : []);
  } catch {
    return new Set();
  }
}

function loadSubOrder() {
  try {
    const parsed = JSON.parse(localStorage.getItem(SUB_ORDER_KEY) || "{}");
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
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
// it. Drag any folder to reorder it among its siblings (top-level folders
// anywhere in the list, subfolders within their parent).
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
    onMoveFolders,
  } = useDisc();
  const [expanded, setExpanded] = useState(loadExpanded);
  const [subOrder, setSubOrder] = useState(loadSubOrder); // { [parentKey]: [rel, ...] }
  const [editingId, setEditingId] = useState(null);
  // Multi-select of top-level folders: shift-click a range, ctrl-click to
  // toggle. Separate from activeFolderId, which only decides what the track
  // list shows.
  const [selectedIds, setSelectedIds] = useState(() => new Set());
  const anchorIdRef = useRef(null);
  const [removeTargets, setRemoveTargets] = useState(null); // folder[] | null
  const inputRef = useRef(null);

  // --- Drag-to-reorder -------------------------------------------------
  // Pointer-driven rather than the browser's built-in drag-and-drop: that
  // gives a fixed ghost image, no auto-scroll, and flickery drop targets.
  // Here a floating label follows the cursor, a line shows exactly where
  // the folder will land, the list scrolls itself near its edges, Escape
  // cancels, and dragging one of several selected folders moves them all.
  const [drag, setDrag] = useState(null); // { group, keys, label, count } | null
  const [dropHint, setDropHint] = useState(null); // { group, key, pos } | null
  const scrollRef = useRef(null);
  const ghostRef = useRef(null);
  const pointerRef = useRef({ x: 0, y: 0 });
  const suppressClickRef = useRef(false);
  const cleanupDragRef = useRef(null);

  useEffect(() => () => cleanupDragRef.current?.(), []);

  useEffect(() => {
    localStorage.setItem(EXPANDED_KEY, JSON.stringify([...expanded]));
  }, [expanded]);

  useEffect(() => {
    localStorage.setItem(SUB_ORDER_KEY, JSON.stringify(subOrder));
  }, [subOrder]);

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

  function beginPress(e, group, key, label) {
    if (e.button !== 0 || editingId) return;
    if (e.target.closest(".sidebar__chevron, .sidebar__remove, input")) return;
    cleanupDragRef.current?.();

    const press = { active: false, keys: [key], hint: null, startX: e.clientX, startY: e.clientY };
    pointerRef.current = { x: e.clientX, y: e.clientY };
    let raf = 0;

    function computeHint(y) {
      const scrollEl = scrollRef.current;
      if (!scrollEl) return null;
      const blocks = [...scrollEl.querySelectorAll(`[data-dnd-group="${group}"]`)];
      if (blocks.length === 0) return null;
      const firstRect = blocks[0].getBoundingClientRect();
      const lastRect = blocks[blocks.length - 1].getBoundingClientRect();
      let target = null;
      let pos = "after";
      if (y <= firstRect.top) {
        // Top-level folders can be dragged to either end from anywhere in
        // the list; a subfolder only counts while near its own siblings.
        if (group !== "top" && y < firstRect.top - 14) return null;
        target = blocks[0];
        pos = "before";
      } else if (y >= lastRect.bottom) {
        if (group !== "top" && y > lastRect.bottom + 14) return null;
        target = blocks[blocks.length - 1];
      } else {
        for (const b of blocks) {
          const r = b.getBoundingClientRect();
          if (y >= r.top && y < r.bottom) {
            target = b;
            pos = y < r.top + r.height / 2 ? "before" : "after";
            break;
          }
        }
      }
      if (!target) return null;
      const targetKey = target.dataset.dndKey;
      if (press.keys.includes(targetKey)) return null;
      return { group, key: targetKey, pos };
    }

    function updateHint() {
      const next = computeHint(pointerRef.current.y);
      const prev = press.hint;
      if (prev?.key === next?.key && prev?.pos === next?.pos) return;
      press.hint = next;
      setDropHint(next);
    }

    function tick() {
      const scrollEl = scrollRef.current;
      if (scrollEl) {
        const r = scrollEl.getBoundingClientRect();
        const y = pointerRef.current.y;
        let dy = 0;
        if (y < r.top + AUTOSCROLL_EDGE_PX) dy = -Math.ceil((r.top + AUTOSCROLL_EDGE_PX - y) / 4);
        else if (y > r.bottom - AUTOSCROLL_EDGE_PX) dy = Math.ceil((y - (r.bottom - AUTOSCROLL_EDGE_PX)) / 4);
        if (dy) {
          scrollEl.scrollTop += dy;
          updateHint();
        }
      }
      raf = requestAnimationFrame(tick);
    }

    function onMove(ev) {
      pointerRef.current = { x: ev.clientX, y: ev.clientY };
      if (!press.active) {
        if (Math.hypot(ev.clientX - press.startX, ev.clientY - press.startY) < DRAG_THRESHOLD_PX) return;
        press.active = true;
        // Dragging one of several selected folders moves the whole selection.
        if (group === "top" && selectedIds.size > 1 && selectedIds.has(key)) {
          press.keys = folders.map((f) => f.id).filter((id) => selectedIds.has(id));
        }
        setDrag({ group, keys: press.keys, label, count: press.keys.length });
        document.body.classList.add("sidebar-dragging");
        raf = requestAnimationFrame(tick);
      }
      if (ghostRef.current) {
        ghostRef.current.style.transform = `translate(${ev.clientX + 12}px, ${ev.clientY + 8}px)`;
      }
      updateHint();
    }

    function finish(apply) {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("keydown", onKey, true);
      cancelAnimationFrame(raf);
      document.body.classList.remove("sidebar-dragging");
      cleanupDragRef.current = null;
      if (press.active) {
        // The click that follows releasing a drag shouldn't also select.
        suppressClickRef.current = true;
        setTimeout(() => {
          suppressClickRef.current = false;
        }, 0);
        const hint = press.hint;
        if (apply && hint) {
          if (group === "top") {
            onMoveFolders(press.keys, hint.key, hint.pos);
          } else {
            const parentKey = group.slice("sub:".length);
            const siblings = [
              ...(scrollRef.current?.querySelectorAll(`[data-dnd-group="${group}"]`) || []),
            ].map((b) => b.dataset.dndKey);
            setSubOrder((prev) => ({
              ...prev,
              [parentKey]: moveKeys(siblings, press.keys, hint.key, hint.pos),
            }));
          }
        }
      }
      setDrag(null);
      setDropHint(null);
    }

    function onUp() {
      finish(true);
    }

    function onKey(ev) {
      if (ev.key === "Escape" && press.active) {
        ev.stopPropagation();
        finish(false);
      }
    }

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("keydown", onKey, true);
    cleanupDragRef.current = () => finish(false);
  }

  function handleFolderClick(e, folder) {
    if (suppressClickRef.current) return;
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

  // Classes + indent for the drop line on a sibling block.
  function blockProps(group, key, depth) {
    const hint = dropHint?.group === group && dropHint.key === key ? dropHint.pos : null;
    const dragging = drag?.group === group && drag.keys.includes(key);
    return {
      "data-dnd-group": group,
      "data-dnd-key": key,
      className:
        "sidebar__block" +
        (hint ? ` sidebar__block--drop-${hint}` : "") +
        (dragging ? " sidebar__block--dragging" : ""),
      style: { "--line-left": `${BASE_PAD_PX + depth * INDENT_PX}px` },
    };
  }

  function renderSubfolders(folderId, parentKey, nodes, depth) {
    const group = `sub:${parentKey}`;
    return applyOrder(nodes, subOrder[parentKey]).map((node) => {
      const key = selectionKey(folderId, node.rel);
      const open = expanded.has(key);
      const hasChildren = node.children.length > 0;
      return (
        <div key={key} {...blockProps(group, node.rel, depth)}>
          <Row
            depth={depth}
            hasChildren={hasChildren}
            open={open}
            onToggle={() => toggle(key)}
            active={activeFolderId === key}
            onSelect={() => {
              if (!suppressClickRef.current) setActiveFolderId(key);
            }}
            name={node.name}
            title={`${node.rel} — drag to reorder`}
            onPointerDown={(e) => beginPress(e, group, node.rel, node.name)}
          />
          {open && hasChildren && (
            <div
              className="sidebar__children"
              style={{ "--guide-x": `${BASE_PAD_PX + depth * INDENT_PX + 8}px` }}
            >
              {renderSubfolders(folderId, key, node.children, depth + 1)}
            </div>
          )}
        </div>
      );
    });
  }

  return (
    <aside className="sidebar">
      <div className="sidebar__scroll" ref={scrollRef} onKeyDown={handleKeyDown}>
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
          return (
            <div key={folder.id} {...blockProps("top", folder.id, 0)}>
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
                  onDoubleClick={() => setEditingId(folder.id)}
                  onPointerDown={(e) => beginPress(e, "top", folder.id, folder.name)}
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
                  {renderSubfolders(folder.id, folder.id, nodes, 1)}
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

      {drag && (
        <div
          className="sidebar__ghost"
          ref={(el) => {
            ghostRef.current = el;
            if (el) {
              el.style.transform = `translate(${pointerRef.current.x + 12}px, ${pointerRef.current.y + 8}px)`;
            }
          }}
        >
          {drag.count > 1 ? `${drag.count} folders` : drag.label}
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
