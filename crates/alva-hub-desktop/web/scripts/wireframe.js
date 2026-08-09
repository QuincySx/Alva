// INPUT:  live DOM (any web frontend — React, Vue, server-rendered, …)
// OUTPUT: wireframe: visual-block list + ASCII blueprint of the page
// POS:    UI review without screenshots. `wireframe()` in the page console,
//         or drive via ui-wireframe.mjs over CDP. Framework-agnostic:
//         semantics come from tags, roles and class names; geometry from
//         getBoundingClientRect.

function wireframe() {
  const SKIP = new Set([
    "SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "LINK", "META", "TITLE",
    "HEAD", "SVG", "PATH", "BR", "HR",
  ]);
  const PANEL_TOKENS = new Set(["panel", "card", "drawer", "sheet", "box", "rail", "dialog", "popover", "menu", "tooltip"]);
  const LAYOUT_CLASSES = new Set(["app-shell", "sidebar", "topbar", "timeline", "composer", "details-drawer", "workspace-panel", "readiness-panel", "fallback-panel", "team-panel", "empty-state", "main-column"]);
  const INTERACTIVE = new Set(["button", "input", "textarea", "select", "a", "label", "option"]);

  const nodes = [];
  let seq = 0;

  function semantic(tag, cls) {
    const c = cls.join(" ");
    if (INTERACTIVE.has(tag)) return tag;
    if (cls.some((k) => LAYOUT_CLASSES.has(k))) return "panel";
    if (cls.some((k) => k.split(/[-_.]/).some((t) => PANEL_TOKENS.has(t)))) return "panel";
    if (tag === "section" || tag === "aside" || tag === "main" || tag === "header" || tag === "form" || tag === "nav") return "container";
    if (c.includes("badge") || c.includes("pill") || c.includes("status")) return "badge";
    if (c.includes("row") || c.includes("item") || c.includes("list")) return "list-item";
    if (c.includes("heading")) return "heading";
    return "el";
  }

  function walk(el, depth, parentId) {
    if (SKIP.has(el.tagName)) return;
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden") return;
    const r = el.getBoundingClientRect();
    if (r.width < 3 || r.height < 3) return;

    const cls = Array.from(el.classList);
    const kind = semantic(el.tagName, cls);
    const label =
      el.getAttribute("aria-label") ||
      (el.tagName === "BUTTON" || el.tagName === "A"
        ? (el.innerText || "").replace(/\s+/g, " ").trim()
        : "") ||
      (el.tagName === "INPUT" || el.tagName === "TEXTAREA"
        ? el.getAttribute("placeholder") || "input"
        : "") ||
      (el.tagName === "SELECT" ? "select" : "") ||
      (el.tagName === "OPTION" ? (el.textContent || "").trim().slice(0, 24) : "");

    const c = cls.join(" ");
    const structural = kind === "panel" || kind === "container" || kind === "badge" || kind === "heading" || kind === "list-item";
    const isLayoutTag = /^(div|section|aside|main|header|form|nav|ul|ol)$/.test(el.tagName);
    // Drop pure wrappers: classless, label-less, non-interactive, non-structural.
    if (isLayoutTag && cls.length === 0 && !label && !structural && el.children.length <= 1) return;
    // Drop OPTION children — the select label suffices.
    if (el.tagName === "OPTION") return;

    const id = seq++;
    nodes.push({
      id,
      parent: parentId,
      depth,
      tag: el.tagName.toLowerCase(),
      kind,
      cls: cls.slice(0, 3).join("."),
      label: label.replace(/\s+/g, " ").trim().slice(0, 44),
      x: Math.round(r.x), y: Math.round(r.y),
      w: Math.round(r.width), h: Math.round(r.height),
    });
    for (const child of el.children) walk(child, depth + 1, id);
  }
  walk(document.body, 0, null);

  // ── Visual-block list, grouped by viewport column ────────────────
  const vw = window.innerWidth;
  const ordered = [...nodes].sort((a, b) => a.y - b.y || a.x - b.x || a.id - b.id);
  const col = (n) => (n.x < vw / 2 ? "L" : "R");

  function describe(n) {
    const box = `${n.x},${n.y} ${n.w}x${n.h}`;
    const label = n.label ? `  "${n.label}"` : "";
    const cls = n.cls ? `  .${n.cls}` : "";
    return `[${n.kind}] ${box}${cls}${label}`;
  }

  // Panels / containers first (the layout skeleton), then controls.
  const skeleton = ordered.filter((n) => n.kind === "panel" || n.kind === "container");
  const controls = ordered.filter(
    (n) => n.kind !== "panel" && n.kind !== "container" && (INTERACTIVE.has(n.tag) || n.kind === "badge" || (n.label && n.kind !== "el")),
  );

  let tree = "";
  let lastCol = null;
  const header = (c) => (c === "L" ? "◤ LEFT ◢" : "◤ RIGHT ◢");
  for (const n of skeleton) {
    if (col(n) !== lastCol) {
      tree += `\n${header(col(n))}\n`;
      lastCol = col(n);
    }
    const indent = n.parent !== null && skeleton.some((s) => s.id === n.parent) ? "  " : "";
    tree += `${indent}${describe(n)}\n`;
  }
  tree += `\n◤ CONTROLS ◢\n`;
  for (const n of controls) tree += `${describe(n)}\n`;

  // ── ASCII blueprint: coarse character grid of panel-level boxes ──
  const W = 100, H = 40;
  const vh = window.innerHeight;
  const sx = W / vw, sy = H / vh;
  const grid = Array.from({ length: H }, () => Array(W).fill(" "));
  const paint = (x, y, ch) => {
    if (x >= 0 && x < W && y >= 0 && y < H) grid[y][x] = ch;
  };
  const boxes = [...skeleton]
    .filter((n) => n.w > 100 && n.h > 30)
    .sort((a, b) => a.y - b.y || a.x - b.x);
  for (const n of boxes) {
    const x1 = Math.max(0, Math.round(n.x * sx)), y1 = Math.max(0, Math.round(n.y * sy));
    const x2 = Math.min(W - 1, Math.round((n.x + n.w) * sx)), y2 = Math.min(H - 1, Math.round((n.y + n.h) * sy));
    if (x2 - x1 < 3 || y2 - y1 < 2) continue;
    for (let x = x1; x <= x2; x++) { paint(x, y1, "─"); paint(x, y2, "─"); }
    for (let y = y1; y <= y2; y++) { paint(x1, y, "│"); paint(x2, y, "│"); }
    paint(x1, y1, "┌"); paint(x2, y1, "┐"); paint(x1, y2, "└"); paint(x2, y2, "┘");
    const tag = n.cls || n.tag;
    if (tag && x2 - x1 > tag.length + 2) {
      for (let i = 0; i < tag.length && x1 + 1 + i < x2; i++) paint(x1 + 1 + i, y1, tag[i]);
    }
  }
  const ascii = grid.map((row) => row.join("").replace(/\s+$/, "")).join("\n");

  return { tree, ascii, viewport: `${vw}x${vh}`, nodeCount: nodes.length };
}
