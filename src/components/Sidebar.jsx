import { useEffect, useRef, useState } from "react";
import { useDisc } from "../context/DiscContext.jsx";
import { parseSelection, isWithin } from "../utils/folderTree.js";
import ConfirmModal from "./ConfirmModal.jsx";
import Icon from "./Icon.jsx";
import "./Sidebar.css";

const EXPANDED_KEY = "lite.expanded";
const INDENT_PX = 14;
const BASE_PAD_PX = 6;
const DRAG_THRESHOLD_PX = 5;
const AUTOSCROLL_EDGE_PX = 40;
const HOVER_EXPAND_MS = 650;

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
function Row({ depth, hasChildren, open, onToggle, active, selected, dropInto, onSelect, name, title, className = "", children, ...rest }) {
  return (
    <div
      className={
        "sidebar__row" +
        (active ? " sidebar__row--active" : "") +
        (selected ? " sidebar__row--selected" : "") +
        (dropInto ? " sidebar__row--drop-into" : "") +
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
//
// Folders are a VS Code–style tree. Drag any folder or subfolder to
// rearrange: the top edge of a row drops above it, the middle drops *into*
// it (nesting it), the bottom edge drops below it. All of this is just how
// Disc arranges things — nothing on disk moves.
export default function Sidebar() {
  const {
    model,
    activeFolderId,
    setActiveFolderId,
    missingFolderIds,
    onAddFolders,
    onRemoveFolders,
    onRenameFolder,
    onMoveNodes,
  } = useDisc();
  const [expanded, setExpanded] = useState(loadExpanded);
  const [editingId, setEditingId] = useState(null);
  // Multi-select of added folders: shift-click a range, ctrl-click to
  // toggle. Separate from activeFolderId, which only decides what the track
  // list shows.
  const [selectedIds, setSelectedIds] = useState(() => new Set());
  const anchorIdRef = useRef(null);
  const [removeTargets, setRemoveTargets] = useState(null); // node[] | null
  const inputRef = useRef(null);
  const modelRef = useRef(model);
  modelRef.current = model;

  // --- Drag and drop -----------------------------------------------------
  // Pointer-driven rather than the browser's built-in drag-and-drop: that
  // gives a fixed ghost image, no auto-scroll, and flickery drop targets.
  // Here a label follows the cursor, a line (or a highlighted row, when
  // dropping *into* a folder) shows exactly where it will land, the list
  // scrolls itself near its edges, and Escape cancels.
  const [drag, setDrag] = useState(null); // { keys, label, count } | null
  const [dropUi, setDropUi] = useState({ line: null, intoKey: null });
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
    if (editingId) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editingId]);

  // Forget selected folders that no longer exist.
  useEffect(() => {
    setSelectedIds((prev) => {
      const next = new Set([...prev].filter((k) => model.nodes.has(k)));
      return next.size === prev.size ? prev : next;
    });
  }, [model]);

  function toggle(key) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  // Every added-folder row currently on screen, top to bottom — what a
  // shift-click range runs over.
  function visibleFolderKeys() {
    const out = [];
    const walk = (parentKey) => {
      for (const node of model.childrenOf.get(parentKey) || []) {
        if (node.kind === "folder") out.push(node.key);
        if (expanded.has(node.key)) walk(node.key);
      }
    };
    walk("");
    return out;
  }

  function beginPress(e, node) {
    if (e.button !== 0 || editingId) return;
    if (e.target.closest(".sidebar__chevron, .sidebar__remove, input")) return;
    cleanupDragRef.current?.();

    const press = { active: false, keys: [node.key], target: null, startX: e.clientX, startY: e.clientY };
    pointerRef.current = { x: e.clientX, y: e.clientY };
    let raf = 0;
    let expandTimer = 0;
    let expandTimerKey = null;

    function computeTarget(y) {
      const scrollEl = scrollRef.current;
      if (!scrollEl) return null;
      const m = modelRef.current;
      const keys = press.keys;
      const sr = scrollEl.getBoundingClientRect();
      const rows = [...scrollEl.querySelectorAll(".sidebar__row[data-node-key]")];
      if (rows.length === 0) return null;
      const rootKids = m.childrenOf.get("") || [];
      // A spot is only valid if it isn't inside what's being dragged.
      const canParent = (p) => p === "" || (!keys.includes(p) && !keys.some((k) => isWithin(m, p, k)));
      const lineAt = (clientY, depth) => ({
        top: clientY - sr.top + scrollEl.scrollTop,
        left: BASE_PAD_PX + depth * INDENT_PX,
      });

      const firstRect = rows[0].getBoundingClientRect();
      const lastRect = rows[rows.length - 1].getBoundingClientRect();
      if (y < firstRect.top) {
        // Above the tree (over All Music / Favorites / the heading): the
        // very first top-level slot.
        return { parentKey: "", beforeKey: rootKids[0]?.key ?? null, intoKey: null, line: lineAt(firstRect.top, 0) };
      }
      if (y >= lastRect.bottom) {
        // Below everything: end of the top level (a way to pull a nested
        // folder back out).
        return { parentKey: "", beforeKey: null, intoKey: null, line: lineAt(lastRect.bottom, 0) };
      }
      const row = rows.find((r) => {
        const b = r.getBoundingClientRect();
        return y >= b.top && y < b.bottom;
      });
      if (!row) return null;
      const key = row.dataset.nodeKey;
      if (keys.includes(key)) return null;
      const depth = Number(row.dataset.depth);
      const b = row.getBoundingClientRect();
      const frac = (y - b.top) / b.height;
      const rowParent = m.effParent.get(key);
      const isOpen = row.dataset.open === "1" && row.dataset.hasKids === "1";

      if (frac < 0.28) {
        if (!canParent(rowParent)) return null;
        return { parentKey: rowParent, beforeKey: key, intoKey: null, line: lineAt(b.top, depth) };
      }
      if (frac > 0.72) {
        if (isOpen) {
          // Below an expanded folder's own row = its first child slot.
          if (!canParent(key)) return null;
          const firstChild = (m.childrenOf.get(key) || [])[0]?.key ?? null;
          return { parentKey: key, beforeKey: firstChild, intoKey: null, line: lineAt(b.bottom, depth + 1) };
        }
        if (!canParent(rowParent)) return null;
        const sibs = m.childrenOf.get(rowParent) || [];
        const i = sibs.findIndex((n) => n.key === key);
        return { parentKey: rowParent, beforeKey: sibs[i + 1]?.key ?? null, intoKey: null, line: lineAt(b.bottom, depth) };
      }
      if (!canParent(key)) return null;
      return { parentKey: key, beforeKey: null, intoKey: key, line: null };
    }

    function updateTarget() {
      const next = computeTarget(pointerRef.current.y);
      const prev = press.target;
      const same =
        prev?.parentKey === next?.parentKey &&
        prev?.beforeKey === next?.beforeKey &&
        prev?.intoKey === next?.intoKey &&
        prev?.line?.top === next?.line?.top &&
        prev?.line?.left === next?.line?.left;
      if (same) return;
      press.target = next;
      setDropUi({ line: next?.line ?? null, intoKey: next?.intoKey ?? null });

      // Hovering over a collapsed folder for a moment opens it, so you can
      // drop deeper inside without letting go first.
      const wantKey = next?.intoKey ?? null;
      if (wantKey !== expandTimerKey) {
        clearTimeout(expandTimer);
        expandTimerKey = wantKey;
        if (wantKey) {
          const row = scrollRef.current?.querySelector(`.sidebar__row[data-node-key="${CSS.escape(wantKey)}"]`);
          if (row && row.dataset.hasKids === "1" && row.dataset.open !== "1") {
            expandTimer = setTimeout(() => {
              setExpanded((p) => new Set(p).add(wantKey));
            }, HOVER_EXPAND_MS);
          }
        }
      }
    }

    function tick() {
      const scrollEl = scrollRef.current;
      if (scrollEl) {
        const r = scrollEl.getBoundingClientRect();
        const y = pointerRef.current.y;
        let dy = 0;
        if (y < r.top + AUTOSCROLL_EDGE_PX) dy = -Math.ceil((r.top + AUTOSCROLL_EDGE_PX - y) / 4);
        else if (y > r.bottom - AUTOSCROLL_EDGE_PX) dy = Math.ceil((y - (r.bottom - AUTOSCROLL_EDGE_PX)) / 4);
        if (dy) scrollEl.scrollTop += dy;
        // Re-check every frame: scrolling, expanding, and layout shifts all
        // move what's under a stationary pointer.
        updateTarget();
      }
      raf = requestAnimationFrame(tick);
    }

    function onMove(ev) {
      pointerRef.current = { x: ev.clientX, y: ev.clientY };
      if (!press.active) {
        if (Math.hypot(ev.clientX - press.startX, ev.clientY - press.startY) < DRAG_THRESHOLD_PX) return;
        press.active = true;
        // Dragging one of several selected folders moves the whole selection.
        if (node.kind === "folder" && selectedIds.size > 1 && selectedIds.has(node.key)) {
          press.keys = visibleFolderKeys().filter((k) => selectedIds.has(k));
        }
        setDrag({ keys: press.keys, label: node.name, count: press.keys.length });
        document.body.classList.add("sidebar-dragging");
        raf = requestAnimationFrame(tick);
      }
      if (ghostRef.current) {
        ghostRef.current.style.transform = `translate(${ev.clientX + 12}px, ${ev.clientY + 8}px)`;
      }
      updateTarget();
    }

    function finish(apply) {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("keydown", onKey, true);
      cancelAnimationFrame(raf);
      clearTimeout(expandTimer);
      document.body.classList.remove("sidebar-dragging");
      cleanupDragRef.current = null;
      if (press.active) {
        // The click that follows releasing a drag shouldn't also select.
        suppressClickRef.current = true;
        setTimeout(() => {
          suppressClickRef.current = false;
        }, 0);
        const t = press.target;
        if (apply && t) {
          onMoveNodes(press.keys, t.parentKey, t.beforeKey);
          // Make sure the moved folder(s) are visible where they landed.
          if (t.parentKey !== "") setExpanded((p) => new Set(p).add(t.parentKey));
        }
      }
      setDrag(null);
      setDropUi({ line: null, intoKey: null });
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

  function handleFolderClick(e, node) {
    if (suppressClickRef.current) return;
    const ids = visibleFolderKeys();
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
      const b = ids.indexOf(node.key);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      setSelectedIds(new Set(ids.slice(lo, hi + 1)));
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      setSelectedIds((prev) => {
        const next = new Set(prev.size === 0 && anchor ? [anchor] : prev);
        if (next.has(node.key)) next.delete(node.key);
        else next.add(node.key);
        return next;
      });
      anchorIdRef.current = node.key;
      return;
    }
    anchorIdRef.current = node.key;
    setSelectedIds(new Set());
    setActiveFolderId(node.key);
  }

  // What × / the Remove button / Delete act on: the whole selection if
  // there is one and it includes the folder in question, else just that one.
  function requestRemove(node) {
    const selectedNodes = () => [...selectedIds].map((k) => model.nodes.get(k)).filter(Boolean);
    if (node && selectedIds.size > 1 && selectedIds.has(node.key)) {
      setRemoveTargets(selectedNodes());
    } else if (node) {
      setRemoveTargets([node]);
    } else if (selectedIds.size > 0) {
      setRemoveTargets(selectedNodes());
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
      setSelectedIds(new Set(visibleFolderKeys()));
    }
  }

  function commitRename(id, value) {
    setEditingId(null);
    onRenameFolder(id, value);
  }

  function renderNodes(parentKey, depth) {
    return (model.childrenOf.get(parentKey) || []).map((node) => {
      const key = node.key;
      const kids = model.childrenOf.get(key) || [];
      const hasChildren = kids.length > 0;
      const open = expanded.has(key);
      const isFolder = node.kind === "folder";
      const missing = isFolder && missingFolderIds.has(key);
      const dragging = drag?.keys.includes(key);
      const dnd = {
        "data-node-key": key,
        "data-depth": depth,
        "data-open": open ? "1" : "0",
        "data-has-kids": hasChildren ? "1" : "0",
        onPointerDown: (e) => beginPress(e, node),
      };

      return (
        <div key={key} className={"sidebar__block" + (dragging ? " sidebar__block--dragging" : "")}>
          {isFolder && editingId === key ? (
            <div className="sidebar__row" style={{ paddingLeft: BASE_PAD_PX + depth * INDENT_PX + 16 }}>
              <input
                ref={inputRef}
                className="sidebar__rename"
                defaultValue={node.name}
                onBlur={(e) => commitRename(key, e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") e.currentTarget.blur();
                  if (e.key === "Escape") setEditingId(null);
                }}
              />
            </div>
          ) : (
            <Row
              depth={depth}
              hasChildren={hasChildren}
              open={open}
              onToggle={() => toggle(key)}
              active={activeFolderId === key}
              selected={isFolder && selectedIds.has(key)}
              dropInto={dropUi.intoKey === key}
              onSelect={(e) => (isFolder ? handleFolderClick(e, node) : !suppressClickRef.current && setActiveFolderId(key))}
              name={node.name}
              className={missing ? "sidebar__row--missing" : ""}
              title={
                isFolder
                  ? missing
                    ? `Can't reach the folder right now`
                    : "Double-click to rename · drag to rearrange · shift/ctrl-click to select several"
                  : `${node.rel} — drag to rearrange`
              }
              onDoubleClick={isFolder ? () => setEditingId(key) : undefined}
              {...dnd}
            >
              {isFolder && (
                <button
                  className="sidebar__remove"
                  title={
                    selectedIds.size > 1 && selectedIds.has(key)
                      ? `Remove all ${selectedIds.size} selected folders from Disc (your files aren't touched)`
                      : "Remove from Disc (your files aren't touched)"
                  }
                  onClick={(e) => {
                    e.stopPropagation();
                    requestRemove(node);
                  }}
                >
                  ×
                </button>
              )}
            </Row>
          )}
          {open && hasChildren && (
            <div className="sidebar__children" style={{ "--guide-x": `${BASE_PAD_PX + depth * INDENT_PX + 8}px` }}>
              {renderNodes(key, depth + 1)}
            </div>
          )}
        </div>
      );
    });
  }

  const hasFolders = (model.childrenOf.get("") || []).length > 0;

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

        {!hasFolders && (
          <button className="sidebar__empty" onClick={onAddFolders}>
            + Add your music folders
          </button>
        )}

        {renderNodes("", 0)}

        {dropUi.line && (
          <div className="sidebar__dropline" style={{ top: dropUi.line.top, left: dropUi.line.left }} />
        )}
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
            onRemoveFolders(removeTargets.map((n) => n.key));
            setSelectedIds(new Set());
            setRemoveTargets(null);
          }}
          onCancel={() => setRemoveTargets(null)}
        />
      )}
    </aside>
  );
}
