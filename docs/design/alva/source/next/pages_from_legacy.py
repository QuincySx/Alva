# INPUT: The legacy JSON packages alva.smallpen (screens, requirements) and alva-foundation.smallpen (components, tokens) plus legacy_flows.json (the `smallpen flow list` output of the alpha.21 CLI for the legacy package, 601 links by page and element name), next to the new package's tokens.css and the Python component model.
# OUTPUT: pages/<module>/<page>[.tablet].html in alva-next.smallpen (data-canvas, data-flow, data-click) and docs/design/alva/requirements/*.md (the requirements and annotations of the legacy package).
# POS: One-time conversion. After it ran, the pages are the package's own source (edit them with the CLI); build_next.py never touches pages/. Remove this script together with the legacy packages and the old source/*.py.
import json
import re
import sys
from collections import OrderedDict, defaultdict
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import build_next as B  # noqa: E402

ALVA = HERE.parents[1]
LEGACY = ALVA / "alva.smallpen"
FOUNDATION = ALVA / "alva-foundation.smallpen"
PKG = ALVA / "alva-next.smallpen"
REQ_OUT = ALVA / "requirements"

# ------------------------------------------------------------------ legacy data
manifest = json.loads((LEGACY / "manifest.json").read_text())
CANVAS = {s: c["name"] for c in manifest["canvases"] for s in c["screens"]}
SCREENS = OrderedDict()
for _f in sorted((LEGACY / "screens").glob("*.json")):
    _s = json.loads(_f.read_text())
    SCREENS[_s["id"]] = _s
SET = {c["id"]: c for c in json.loads((FOUNDATION / "components/components.json").read_text())["componentSets"]}
OLD_NODES = {}  # component id -> node id -> node
for _c in SET.values():
    d = {}
    for v in _c["variants"]:
        for n in v["nodes"].values():
            d[n["id"]] = n
    OLD_NODES[_c["id"]] = d
ANCESTORS = {}  # component id -> node id -> names from the root's child down to the node
for _c in SET.values():
    d = {}
    for v in _c["variants"]:
        def walk(nid, chain, v=v, d=d):
            node = v["nodes"][nid]
            if chain is not None:
                d.setdefault(nid, [*chain, node["name"]])
            for ch in node.get("children", []):
                walk(ch, [*(chain or []), node["name"]] if chain is not None else [], v=v, d=d)
        walk(v["rootId"], None)
    ANCESTORS[_c["id"]] = d
AXES = {cid: {a["id"]: a["name"] for a in c["axes"]} for cid, c in SET.items()}

_tok = json.loads((FOUNDATION / "tokens/tokens.json").read_text())
TOKEN_NAME, TOKEN_SETS = {}, {}
for _set in _tok["sets"]:
    TOKEN_SETS[_set["name"]] = {t["name"]: t["value"] for t in _set["tokens"]}
    for t in _set["tokens"]:
        TOKEN_NAME[t["id"]] = t["name"]
DEFAULT_SETS = ["Theme/Default", "Appearance/Light", "Viewport/Desktop", "Density/Comfortable"]

# flows: page -> element label -> [targets] in listing order
def load_flows(path):
    text = json.loads(Path(path).read_text())["text"]
    lines = [l.strip() for l in text.splitlines()]
    links = defaultdict(list)
    starts = []
    mode = None
    for l in lines:
        if l == "Starts":
            mode = "s"
        elif l == "Links":
            mode = "l"
        elif mode == "s" and l:
            starts.append(l)
        elif mode == "l" and l:
            m = re.match(r"^(.*?) \((\w+)\) / (.*?) —(\w+)→ (.*)$", l)
            links[(m.group(1), m.group(3))].append(m.group(5))
    return links, starts


def resolve_value(name, depth=0):
    for s in reversed(DEFAULT_SETS):
        if name in TOKEN_SETS.get(s, {}):
            v = TOKEN_SETS[s][name]
            if isinstance(v, str) and v.startswith("{") and depth < 6:
                return resolve_value(v[1:-1], depth + 1)
            return v
    return None


def token_ref(binding):
    """binding {'assetId': id} -> ('{ui.x}' ref) for a ui.* token present in the new package, or ('lit', value)."""
    if not binding:
        return None
    name = TOKEN_NAME.get(binding.get("assetId"))
    if name is None:
        return None
    if name.startswith("ui.") and name[3:] in B.CSS:
        return "{" + name + "}"
    value = resolve_value(name)
    return ("lit", value) if isinstance(value, (str, int, float)) else None


# ---------------------------------------------------------------- type tokens
TYPE_LOOKUP = {}
for _m in re.finditer(r'--(type-[\w-]+): (\d+) ([\d.]+)px/([\d.]+) "([^"]+)";', (PKG / "tokens.css").read_text()):
    TYPE_LOOKUP[(_m.group(5), int(_m.group(2)), float(_m.group(3)), float(_m.group(4)))] = _m.group(1)
FAMILIES = {"Alva UI Sans SC", "JetBrains Mono", "Alva Interface Icons"}

REPORT = defaultdict(list)  # category -> messages


def note(kind, msg):
    REPORT[kind].append(msg)


def hexcls(prefix, color):
    c = color.upper()
    return f"{prefix}-[{c}]"


def paint_class(prefix, ref_or_lit, color=None):
    if isinstance(ref_or_lit, str) and ref_or_lit.startswith("{ui."):
        return f"{prefix}-{B.suffix(ref_or_lit)}"
    if isinstance(ref_or_lit, tuple):
        return hexcls(prefix, ref_or_lit[1])
    if color:
        return hexcls(prefix, color)
    return None


def bound_value(tb, key):
    """Current value of the ui.* token a binding names, or None (the saved literal may be stale; the Token wins)."""
    ref = token_ref((tb or {}).get(key))
    if isinstance(ref, str):
        return B.TOK[ref[4:-1]]["value"]
    return None


def text_classes(n):
    ts, tb = n["textStyle"], n.get("tokenBindings") or {}
    fam = bound_value(tb, "fontFamily") or ts.get("fontFamily", "Alva UI Sans SC")
    if fam not in FAMILIES:
        fam = "Alva UI Sans SC"
    size = float(bound_value(tb, "fontSize") or ts.get("fontSize", 14))
    weight = int(bound_value(tb, "fontWeight") or ts.get("fontWeight", 400))
    line = bound_value(tb, "lineHeight") or ts.get("lineHeight")
    key = (fam, weight, size, float(line) if line is not None else 1.2)
    out = []
    if key in TYPE_LOOKUP:
        out.append(TYPE_LOOKUP[key])
    else:
        out += [f"font-[{fam.replace(' ', '_')}]", f"text-[{B.num(size)}px]", f"font-[{weight}]", f"leading-[{key[3]}]"]
    if ts.get("textAlign") in ("center", "right"):
        out.append("text-" + ts["textAlign"])
    return out


def style_override_classes(ts):
    """A page override of a text part's style: an existing type Token when the style matches one, else literal size/weight."""
    fam = ts.get("fontFamily", "Alva UI Sans SC")
    if fam not in FAMILIES:
        fam = "Alva UI Sans SC"
    size, weight = float(ts.get("fontSize", 14)), int(ts.get("fontWeight", 400))
    line = float(ts["lineHeight"]) if ts.get("lineHeight") is not None else None
    for l in ([line] if line is not None else []) + [1.5, 1.0]:
        key = (fam, weight, size, l)
        if key in TYPE_LOOKUP:
            return [TYPE_LOOKUP[key]]
    return [f"text-[{B.num(size)}px]", f"font-[{weight}]"]


def color_of(n, tokens):
    ref = token_ref((n.get("tokenBindings") or {}).get("fill"))
    fills = n.get("fills") or []
    color = fills[0].get("color") if fills and fills[0].get("type") == "solid" else None
    return ref, color


# ------------------------------------------------------------------- emission
def esc(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def attr(v):
    return str(v).replace("&", "&amp;").replace('"', "&quot;")


def text_html(t):
    if t == "":
        return "&#8203;"
    return "<br>".join(esc(x) for x in t.split("\n"))


class Page:
    def __init__(self, screen, pres):
        self.screen, self.pres = screen, pres
        self.name = screen["name"]
        self.canvas = CANVAS[screen["id"]]
        self.nodes = pres["nodes"]
        self.labels = {}  # node id -> label (names joined by ' / ')
        self.paths = {}  # node id -> data-name chain in the new page

    def flow_label(self, chain):
        return " / ".join(chain)


class Converter:
    def __init__(self, links):
        self.links = {k: list(v) for k, v in links.items()}
        self.used_links = 0
        self.click_count = defaultdict(int)
        self.back_count = 0
        self.cross = []
        self.uuid_votes = defaultdict(set)
        self.uuid_map = {}
        self.extra = []

    # ---- sizing classes
    def dim(self, axis, n, parent_hug):
        key = "layout-item-h-sizing" if axis == "w" else "layout-item-v-sizing"
        sizing = n.get(key)
        tb = n.get("tokenBindings") or {}
        if sizing == "fill":
            return [f"{axis}-fit" if parent_hug else f"{axis}-full"]
        if sizing == "auto":
            return [f"{axis}-fit"]
        ref = token_ref(tb.get("width" if axis == "w" else "height"))
        if isinstance(ref, str) and ref.startswith("{ui.") and B.TOK[ref[4:-1]]["type"] == "sizing":
            return [f"{axis}-{B.suffix(ref)}"]
        v = n["width" if axis == "w" else "height"]
        return [f"{axis}-[{B.num(v)}px]"]

    def dest(self, page, interaction, chain):
        if interaction["action-type"] == "prev-screen":
            self.back_count += 1
            return "back"
        target = None
        for k in range(1, len(chain) + 1):  # the legacy listing names an element by its shortest unique name suffix
            label = " / ".join(chain[-k:])
            lst = self.links.get((page.name, label))
            if lst:
                target = lst.pop(0)
                break
        if target is None and interaction.get("destination") in self.uuid_map:
            target = self.uuid_map[interaction["destination"]]
        if target is None:
            note("unresolved-link", f"{page.name} / {' / '.join(chain)}")
            return None
        self.used_links += 1
        self.uuid_votes[interaction.get("destination")].add(target)
        tc = CANVAS_BY_NAME.get(target)
        if tc and tc != page.canvas:
            self.cross.append((page.name, label, target))
        return page_ref(target)

    def clicks(self, page, n, chain):
        out = []
        for i in n.get("interactions") or []:
            d = self.dest(page, i, chain)
            if d:
                out.append(d)
        return out

    # ---- frame
    def frame_classes(self, n, parent_hug_w, parent_hug_h, root=False):
        tb = n.get("tokenBindings") or {}
        cls = ["flex", "flex-row" if n.get("layout-flex-dir", "row").startswith("row") else "flex-col"]
        if n.get("layout-flex-dir", "").endswith("reverse"):
            cls[1] += "-reverse"
        if not root:
            cls += self.dim("w", n, parent_hug_w) + self.dim("h", n, parent_hug_h)
        ai = n.get("layout-align-items", "start")
        cls.append("items-" + {"flex-start": "start", "flex-end": "end"}.get(ai, ai))
        jc = n.get("layout-justify-content", "start")
        cls.append("justify-" + {"space-between": "between", "space-around": "around", "space-evenly": "evenly", "flex-start": "start", "flex-end": "end"}.get(jc, jc))
        gap = (n.get("layout-gap") or {})
        horizontal = n.get("layout-flex-dir", "row").startswith("row")
        gv = gap.get("column-gap" if horizontal else "row-gap", 0)
        gref = token_ref(tb.get("itemSpacing"))
        if gv:
            cls.append("gap-" + (B.suffix(gref) if isinstance(gref, str) else B.spacing(gv)))
        pad = n.get("layout-padding")
        if pad:
            vals = [pad.get("p1", 0), pad.get("p2", 0), pad.get("p3", 0), pad.get("p4", 0)]
            for i, key in enumerate(("paddingTop", "paddingRight", "paddingBottom", "paddingLeft")):
                ref = token_ref(tb.get(key))
                if isinstance(ref, str) and vals[i]:
                    vals[i] = ref
            if any(vals):
                cls += B.spaced("p", vals, False)
        margin = n.get("layout-item-margin")
        if margin:
            vals = [margin.get("m1", 0), margin.get("m2", 0), margin.get("m3", 0), margin.get("m4", 0)]
            for i, key in enumerate(("marginTop", "marginRight", "marginBottom", "marginLeft")):
                ref = token_ref(tb.get(key))
                if isinstance(ref, str) and vals[i]:
                    vals[i] = ref
            if any(vals):
                cls += B.spaced("m", vals, False)
        r = n.get("cornerRadius")
        if r:
            rref = token_ref(tb.get("cornerRadius"))
            cls.append(B.radius_class(rref) if isinstance(rref, str) else B.radius_class(r))
        ref, color = color_of(n, tb)
        fills = n.get("fills") or []
        if fills and fills[0].get("type") == "solid":
            c = paint_class("bg", ref, color)
            if c:
                cls.append(c)
        for s in (n.get("strokes") or [])[:1]:
            sref = token_ref(tb.get("stroke"))
            color_cls = paint_class("border", sref, s.get("color"))
            width = s.get("width", 1)
            pos = {"inner": "inside", "center": "center", "outer": "outside"}.get(s.get("alignment", "inner"), "inside")
            if color_cls:
                cls += ["border" if width == 1 else f"border-[{B.num(width)}px]", color_cls, "border-" + pos]
        sh = token_ref(tb.get("shadow"))
        if isinstance(sh, str):
            cls.append("shadow-" + B.suffix(sh))
        mw = n.get("layout-item-max-w")
        if mw and n.get("layout-item-h-sizing") == "fill" and not parent_hug_w:
            cls.append(f"max-w-[{B.num(mw)}px]")
        if n.get("visible") is False:
            cls.append("hidden")
        return cls

    def emit(self, page, nid, indent, parent, chain, root=False):
        n = page.nodes[nid]
        pad = "  " * indent
        name = n["name"]
        uname = self.unique(page, parent, name) if parent else name
        mychain = chain + ([] if root else [name])
        page.labels[nid] = mychain
        phug_w = parent is not None and parent["hug_w"]
        phug_h = parent is not None and parent["hug_h"]
        if n["type"] == "TEXT":
            force = None
            if parent and parent.get("flip"):
                force = "w-fit" if parent["idx"] == 0 else "w-full"
                parent["idx"] += 1
            return [self.emit_text(page, n, pad, phug_w, mychain, uname, force)]
        if n["type"] == "INSTANCE":
            return self.emit_instance(page, n, pad, phug_w, phug_h, mychain, uname)
        cls = self.frame_classes(n, phug_w, phug_h, root)
        flip = self.header_copy(page, n, parent)
        if flip:
            cls = [c for c in cls if not c.startswith(("flex-col", "items-", "gap-"))] + ["flex-row", "items-center", "gap-2"]
        clicks = self.clicks(page, n, mychain)
        info = {"hug_w": n.get("layout-item-h-sizing") == "auto" or (n.get("layout-item-h-sizing") == "fill" and phug_w),
                "hug_h": n.get("layout-item-v-sizing") == "auto" or (n.get("layout-item-v-sizing") == "fill" and phug_h)}
        if root:
            info = {"hug_w": False, "hug_h": False}
        tag = "body" if root else "div"
        head = f'{pad}<{tag}'
        if root:
            head += self.body_attrs(page)
        else:
            head += f' data-name="{attr(uname)}"'
        if clicks:
            head += f' data-click="{attr(clicks[0])}"'
        head += f' class="{" ".join(cls)}">'
        lines = [head]
        seen = set()
        is_header = "h-header" in cls
        for child in reversed(n.get("children", [])):  # Penpot stores the last visual child first
            lines += self.emit(page, child, indent + 1, {**info, "names": seen, "header": is_header, "flip": flip, "idx": 0}, mychain)
        lines.append(f"{pad}</{tag}>")
        return lines

    def header_copy(self, page, n, parent):
        """The 36px toolbar row (size.header) holds title and source on one line; a stacked legacy copy would not fit."""
        if not parent or not parent.get("header"):
            return False
        kids = [page.nodes[c] for c in n.get("children", [])]
        if n.get("layout-flex-dir") == "column" and len(kids) == 2 and all(k["type"] == "TEXT" for k in kids):
            note("header-copy-row", f"{page.name}: {n['name']}")
            return True
        return False

    def body_attrs(self, page):
        out = f' data-canvas="{attr(page.canvas)}"'
        flows = page.pres.get("prototypeFlows") or []
        if flows:
            out += f' data-flow="{attr(flows[0]["name"])}"'
        return out

    def unique(self, page, parent, name):
        names = parent["names"]
        base, k = name, 2
        while name in names:
            name = f"{base}{k}"
            k += 1
        names.add(name)
        return name

    def emit_text(self, page, n, pad, phug_w, chain, uname, force=None):
        tb = n.get("tokenBindings") or {}
        cls = []
        sizing = n.get("layout-item-h-sizing")
        if sizing == "fill":
            cls.append("w-fit" if phug_w else "w-full")
        elif sizing == "auto" or n.get("growType") == "auto-width":
            cls.append("w-fit")
        else:
            cls.append(f"w-[{B.num(n['width'])}px]")
        if force:
            cls[0] = force
        cls.append("h-fit")
        cls += text_classes(n)
        ref, color = color_of(n, tb)
        c = paint_class("text", ref, color)
        if c:
            cls.append(c)
        if "\n" in n["text"]:
            cls.append("whitespace-pre-wrap")
        if n.get("layout-item-max-w") and sizing == "fill" and not phug_w:
            cls.append(f"max-w-[{B.num(n['layout-item-max-w'])}px]")
        if n.get("visible") is False:
            cls.append("hidden")
        name = uname
        content = text_html(n["text"])
        clicks = self.clicks(page, n, chain)
        click = f' data-click="{attr(clicks[0])}"' if clicks else ""
        return f'{pad}<p data-name="{attr(name)}"{click} class="{" ".join(cls)}">{content}</p>'

    # ---- instances
    def emit_instance(self, page, n, pad, phug_w, phug_h, chain, uname):
        inst = n["instance"]
        cid = inst["component"]["assetId"]
        comp = SET[cid]
        legacy = not comp["name"].startswith("Alva ")
        axes = AXES[cid]
        variant = {axes.get(k, k): v for k, v in inst["variant"].items()}
        ov = B.new_ov()
        self.extra = []
        if legacy:
            newname, props = self.legacy(comp["name"], variant, inst, ov, page, chain)
        else:
            newname = comp["name"]
            props = dict(variant)
            self.alva_overrides(page, n, inst, cid, ov, chain)
        newbase = newname.removeprefix("Alva ")
        if newname not in B.DEFS:
            note("missing-component", f"{page.name}: {comp['name']}")
            return []
        for ax, val in list(props.items()):
            dom = B.DEFS[newname]["properties"].get(ax)
            if dom is None or val not in dom:
                note("variant-unmapped", f"{page.name}: {comp['name']} {ax}={val}")
                del props[ax]
        cls = []
        for axis in ("w", "h"):
            c = self.dim(axis, n, phug_w if axis == "w" else phug_h)
            cls += c
        if n.get("layout-item-max-w") and n.get("layout-item-h-sizing") == "fill" and not phug_w:
            cls.append(f"max-w-[{B.num(n['layout-item-max-w'])}px]")
        clicks = self.clicks(page, n, chain)
        cls += self.extra
        if n.get("visible") is False:
            cls.append("hidden")
        click = f' data-click="{attr(clicks[0])}"' if clicks else ""
        pr = B.props_attr(props)
        tag = f'{pad}<sp-use component="{newbase}" data-name="{attr(uname)}"{pr}{click} class="{" ".join(cls)}"'
        inner = self.emit_ov(ov["kids"], len(pad) // 2 + 1)
        if inner:
            return [tag + ">", *inner, f"{pad}</sp-use>"]
        return [tag + "></sp-use>"]

    def old_chain(self, cid, key):
        """Names of the nodes an override key addresses (ancestor frames included), plus the old node of the last id."""
        names, comp, node = [], cid, None
        for nid in key.split("__"):
            node = OLD_NODES[comp].get(nid)
            if node is None:
                return None, None
            names += ANCESTORS[comp].get(nid, [node["name"]])
            if node["type"] == "INSTANCE":
                comp = node["instance"]["component"]["assetId"]
        return names, node

    def alva_overrides(self, page, n, inst, cid, ov, chain):
        defname = SET[cid]["name"]
        by_target = defaultdict(dict)
        for key, value in (inst.get("overrides") or {}).items():
            path, field = key.rsplit(":", 1)
            by_target[path][field] = value
        for path, fields in by_target.items():
            names, node = self.old_chain(cid, path)
            if names is None:
                note("override-unmapped", f"{page.name}: {n['name']} {path}")
                continue
            self.apply_fields(page, defname, names, node, fields, ov, chain)

    def apply_fields(self, page, defname, names, node, fields, ov, chain):
        if len(names) == 1 and names[0] == defname.removeprefix("Alva "):
            # an override of the copy's own root: classes on the copy itself
            tb = fields.get("tokenBindings") or {}
            if "fills" in fields or tb.get("fill"):
                ref = token_ref(tb.get("fill"))
                color = fields["fills"][0].get("color") if fields.get("fills") else None
                c = paint_class("bg", ref, color)
                if c:
                    self.extra.append(c)
            if fields.get("cornerRadius"):
                self.extra.append(B.radius_class(fields["cornerRadius"]))
            strokes = fields.get("strokes")
            if strokes and strokes[0].get("opacity") == 0:
                self.extra.append("border-transparent")  # an invisible stroke override: the copy's own border disappears
            for f in fields:
                if f not in ("fills", "tokenBindings", "cornerRadius", "width", "height", "strokes"):
                    note("override-dropped", f"{page.name}: {defname} root.{f}")
            return

        def put(field, value):
            try:
                B.ov_set(ov, defname, names, field, value)
            except KeyError:
                note("override-unmapped", f"{page.name}: {defname} {'/'.join(names)}.{field}")
        for field, value in fields.items():
            if field == "text":
                put("text", value)
            elif field == "variant":
                comp = node["instance"]["component"]["assetId"]
                put("props", {AXES[comp].get(k, k): v for k, v in value.items()})
            elif field == "visible":
                if isinstance(value, bool):
                    put("visible", value)
            elif field == "width":
                put("width", value)
            elif field == "interactions":
                for i in value:
                    d = self.dest(page, i, chain + names)
                    if d:
                        put("click", d)
            elif field == "textStyle":
                tb = fields.get("tokenBindings") or {}  # a bound size/weight/line wins over the saved literal (which can be stale)
                ts = dict(value)
                for key, tkey in (("fontSize", "fontSize"), ("fontWeight", "fontWeight"), ("lineHeight", "lineHeight")):
                    bound = bound_value(tb, tkey)
                    if bound is not None:
                        ts[key] = bound
                cls = style_override_classes(ts)
                if cls:
                    put("cls", cls)
            elif field == "height" and names[-1] != "Glyph":
                put("height", value)
            elif field == "fills" or (field == "tokenBindings" and "fills" not in fields and (value or {}).get("fill")):
                ref = token_ref((fields.get("tokenBindings") or {}).get("fill"))
                color = fields["fills"][0].get("color") if fields.get("fills") else None
                val = ref if ref else color
                if val:
                    put("color" if node["type"] == "TEXT" else "fill", val)
            elif field == "tokenBindings":
                pass
            else:
                note("override-dropped", f"{page.name}: {defname} {'/'.join(names)}.{field}")

    LEGACY_ICON = {"◷": "Clock", "◉": "Chat", "⌘": "Terminal", "▤": "List", "◇": "Plugin", "⚙": "Settings", "◎": "Target", "◌": "Folder"}

    def legacy(self, name, variant, inst, ov, page, chain):
        cid = inst["component"]["assetId"]
        by_target = defaultdict(dict)
        for key, value in (inst.get("overrides") or {}).items():
            path, field = key.rsplit(":", 1)
            by_target[path][field] = value
        texts = {}
        extra = []
        for path, fields in by_target.items():
            names, node = self.old_chain(cid, path)
            if names is None:
                continue
            tail = names[-1]
            for field, value in fields.items():
                if field == "text":
                    texts[tail] = value
                elif field == "interactions":
                    for i in value:
                        d = self.dest(page, i, chain + names)
                        if d:
                            extra.append((tail, "click", d))
                elif field == "variant":
                    extra.append((tail, "variant", {AXES[node["instance"]["component"]["assetId"]].get(k, k): v for k, v in value.items()}))
                elif field == "visible" and isinstance(value, bool):
                    extra.append((tail, "visible", value))
                elif field in ("width", "tokenBindings", "fills", "textStyle", "height", "cornerRadius", "strokes"):
                    if not (name == "StatusBadge" and field == "width") and not (field == "tokenBindings"):
                        note("override-dropped", f"{page.name}: legacy {name}.{tail}.{field}")
        def put(parts, field, value):
            B.ov_set(ov, "Alva " + target, parts, field, value)
        if name == "ActionButton":
            target = "Button"
            props = {"Kind": variant.get("Style", "Secondary"), "State": variant.get("State", "Default"), "Content": "Text"}
            if "Label" in texts:
                put(["Label"], "text", texts["Label"])
        elif name == "StatusBadge":
            target = "Badge"
            props = {"Tone": variant.get("Tone", "Neutral")}
            if "Label" in texts:
                put(["Label"], "text", texts["Label"])
        elif name == "Tab":
            target = "Tab"
            props = {"State": variant.get("State", "Default")}
            if "Label" in texts:
                put(["Label"], "text", texts["Label"])
        elif name == "NavigationItem":
            target = "NavItem"
            props = {"State": variant.get("State", "Default")}
            if "Label" in texts:
                put(["Label"], "text", texts["Label"])
            if "Icon" in texts:
                glyph = self.LEGACY_ICON.get(texts["Icon"])
                if glyph:
                    put(["Icon"], "props", {"Name": glyph})
                else:
                    note("variant-unmapped", f"{page.name}: NavigationItem icon {texts['Icon']!r}")
            for tail, field, value in extra:
                if field == "visible" and tail == "Icon":
                    put(["Icon"], "visible", value)
        elif name == "TextField":
            target = "Input"
            state = {"Focused": "Focus"}.get(variant.get("State", "Default"), variant.get("State", "Default"))
            props = {"Kind": "Text", "State": state}
            if "Label" in texts:
                put(["Value"], "text", texts["Label"])
        elif name == "SettingRow":
            target = "SettingRow"
            props = {"Control": "Switch"}
            if "Title" in texts:
                put(["Title"], "text", texts["Title"])
            if "Description" in texts:
                put(["Description"], "text", texts["Description"])
            for tail, field, value in extra:
                if field == "variant" and tail == "Switch":
                    put(["Control"], "props", {"State": value.get("State", "Off")})
        elif name == "ChoiceOption":
            target = "QuestionOption"
            props = {"State": variant.get("State", "Default")}
            if "Label" in texts:
                put(["Title"], "text", texts["Label"])
            if variant.get("Kind") == "Multiple":
                note("variant-unmapped", f"{page.name}: ChoiceOption Kind=Multiple")
        elif name == "ToolActivity":
            target = "ToolActivity"
            props = {"State": "Collapsed"}
        elif name == "Feedback":
            target = "EmptyState"
            props = {"State": variant.get("State", "Empty")}
        elif name == "Toggle":
            target = "Switch"
            props = {"State": variant.get("State", "Off")}
        else:
            note("missing-component", f"{page.name}: legacy {name}")
            target = None
            props = {}
        return ("Alva " + target if target else name), props

    # ---- override tree emission (page flavour)
    def emit_ov(self, kids, indent):
        pad = "  " * indent
        lines = []
        for name, e in kids.items():
            a = e["attrs"]
            cls = []
            if e["kind"] == "text":
                node = B.NODES[e["nid"]]
                if "color" in a:
                    cls.append(self.paint("text", a["color"]))
            elif "fill" in a:
                cls.append(self.paint("bg", a["fill"]))
            if "width" in a:
                cls.append(f"w-[{B.num(a['width'])}px]")
            if "height" in a:
                cls.append(f"h-[{B.num(a['height'])}px]")
            cls += a.get("cls", [])
            if a.get("visible") is False:
                cls.append("hidden")
            elif a.get("visible") is True:
                cls.append("shown")
            cls = [c for c in cls if c]
            click = f' data-click="{attr(a["click"])}"' if a.get("click") else ""
            if e["kind"] == "text":
                hidden = a.get("visible") is False
                content = "" if hidden or "text" not in a else text_html(a["text"])
                lines.append(f'{pad}<p data-name="{attr(name)}"{click} class="{" ".join(cls)}">{content}</p>'.replace(' class=""', ""))
            elif e["kind"] == "use":
                inner = self.emit_ov(e["kids"], indent + 1)
                # no base props here: the copy's props depend on the variant of the surrounding copy, so only explicit overrides are written
                tag = f'{pad}<sp-use data-name="{attr(name)}"{B.props_attr(a.get("props"))}{click}{(" class=" + chr(34) + " ".join(cls) + chr(34)) if cls else ""}'
                lines += [tag + ">", *inner, f"{pad}</sp-use>"] if inner else [tag + "></sp-use>"]
            else:
                inner = self.emit_ov(e["kids"], indent + 1)
                lines += [f'{pad}<div data-name="{attr(name)}"{click}{(" class=" + chr(34) + " ".join(cls) + chr(34)) if cls else ""}>', *inner, f"{pad}</div>"]
        return lines

    @staticmethod
    def paint(prefix, v):
        if isinstance(v, str) and v.startswith("{ui."):
            return f"{prefix}-{B.suffix(v)}"
        if isinstance(v, tuple):
            return hexcls(prefix, v[1])
        if isinstance(v, str) and v.startswith("#"):
            return hexcls(prefix, v)
        return None


def write_requirements(registry):
    """The legacy requirements (and the annotations bound to nodes) become Markdown beside the package; pages get a pointer comment."""
    REQ_OUT.mkdir(exist_ok=True)
    pointers = defaultdict(set)  # page rel path -> requirement ids
    stamp = lambda rel, label: f"`{rel}`" + (f" → `{label}`" if label else " → 页面根")
    narrow_note = " （22 号的 960 窄窗版本，不是平板设备）"
    for f in sorted((LEGACY / "requirements").glob("*.json")):
        doc = json.loads(f.read_text())
        lines = [f"# {f.stem}", "", f"> 来源：旧包 `alva.smallpen/requirements/{f.name}`。本文件是这些需求的正本；页面位置按新包路径和元素 `data-name` 链给出。", ""]
        by_req = defaultdict(list)
        for a in doc["annotations"]:
            m = re.match(r"^(req_[\w]+)", a["description"])
            by_req[m.group(1) if m else ""].append(a)
        def where(target):
            rel, label = registry.get((target["presentationId"], target["nodeId"]), (None, None))
            if rel is None:
                return "（旧节点已不在新包页面里）"
            tag = narrow_note if rel.endswith(".tablet.html") else ""
            return stamp(rel, label) + tag
        for r in doc["requirements"]:
            lines += [f"## {r['id']} — {r['title']}", "", r["markdown"].rstrip(), ""]
            if r["links"]:
                lines.append("绑定位置：")
                for l in r["links"]:
                    lines.append(f"- {where(l)}")
                    rel = registry.get((l["presentationId"], l["nodeId"]), (None, None))[0]
                    if rel:
                        pointers[rel].add(r["id"])
                lines.append("")
            anns = by_req.get(r["id"], [])
            if anns:
                lines.append("标注：")
                for a in anns:
                    lines.append(f"- [{a.get('role', '')}] {a['description']} — {where(a['target'])}")
                    rel = registry.get((a["target"]["presentationId"], a["target"]["nodeId"]), (None, None))[0]
                    if rel:
                        pointers[rel].add(r["id"])
                lines.append("")
        rest = [a for k, v in by_req.items() if k not in {r["id"] for r in doc["requirements"]} for a in v]
        if rest:
            lines += ["## 其他标注", ""]
            for a in rest:
                lines.append(f"- [{a.get('role', '')}] {a['description']} — {where(a['target'])}")
            lines.append("")
        (REQ_OUT / f"{f.stem}.md").write_text("\n".join(lines), encoding="utf-8")
    for rel, ids in pointers.items():
        path = PKG / rel
        text = path.read_text(encoding="utf-8")
        note_text = "需求与标注见 docs/design/alva/requirements/*.md：" + "、".join(sorted(ids))
        if rel.endswith(".tablet.html"):
            note_text += "（这是 960 窄窗版本，不是平板设备）"
        text = re.sub(r"(<body[^>]*>)", lambda m: m.group(1) + f"\n  <!-- {note_text} -->", text, count=1)
        path.write_text(text, encoding="utf-8")


def page_ref(name):
    mod, _, page = name.partition(" / ")
    return f"{mod}/{page}"


CANVAS_BY_NAME = {s["name"]: CANVAS[s["id"]] for s in SCREENS.values()}


def page_file(screen, pres, base_pres):
    mod, _, page = screen["name"].partition(" / ")
    suffix = "" if pres is base_pres else ".tablet"
    return PKG / "pages" / mod / f"{page}{suffix}.html"


def main():
    args = [a for a in sys.argv[1:] if a != "--force"]
    if (PKG / "pages").exists() and "--force" not in sys.argv:
        sys.exit("alva-next.smallpen/pages already exists and is now the source of truth; --force overwrites every page (and any edit made since)")
    flows_path = args[0] if args else str(HERE / "legacy_flows.json")
    out_dir = Path(args[1]) if len(args) > 1 else None
    links, starts = load_flows(flows_path)
    learn = Converter(links)  # first pass only learns which destination id is which page
    convert_all(learn, None)
    uuid_map = {u: next(iter(t)) for u, t in learn.uuid_votes.items() if len(t) == 1}
    REPORT.clear()
    conv = Converter(links)
    conv.uuid_map = uuid_map
    convert_all(conv, out_dir)


def convert_all(conv, out_dir):
    registry = {}  # (presentation id, node id) -> (file relative path, label)
    count = 0
    for screen in SCREENS.values():
        base = next(p for p in screen["presentations"] if p["id"] == screen["basePresentationId"])
        for pres in screen["presentations"]:
            page = Page(screen, pres)
            if pres is not base:
                note("narrow-presentation", f"{screen['name']} ({pres['platform']} {pres['viewport']})")
            root = pres["nodes"][pres["rootId"]]
            body = conv.emit(page, pres["rootId"], 0, None, [], root=True)
            w, h = pres["viewport"]["width"], pres["viewport"]["height"]
            # the root frame is the body: give it the viewport size
            vw = "w-viewport-width" if (w == 1440 and CANVAS[screen["id"]] == "01 产品界面") else f"w-[{w}px]"
            vh = "h-viewport-height" if (h == 960 and CANVAS[screen["id"]] == "01 产品界面") else f"h-[{h}px]"
            body[0] = body[0].replace('class="', f'class="{vw} {vh} ', 1)
            path = page_file(screen, pres, base)
            rel = path.relative_to(PKG)
            depth = len(rel.parts) - 1
            head = f'<!doctype html>\n<html>\n<head>\n  <meta charset="utf-8">\n  <title>{esc(screen["name"])}</title>\n  <link rel="stylesheet" href="{"../" * depth}tokens.css">\n</head>\n'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(head + "\n".join(body) + "\n</html>\n", encoding="utf-8")
            count += 1
            for nid, chain in page.labels.items():
                registry[(pres["id"], nid)] = (str(rel), " / ".join(chain))
    write_requirements(registry)
    if out_dir is None:
        return
    (out_dir / "legacy_registry.json").write_text(json.dumps({f"{k[0]}|{k[1]}": v for k, v in registry.items()}, ensure_ascii=False))
    leftover = sum(len(v) for v in conv.links.values())
    bad = {u: t for u, t in conv.uuid_votes.items() if len(t) > 1}
    print("destination ids with more than one page:", len(bad), list(bad.items())[:3])
    print(f"{count} pages; links resolved {conv.used_links}, back {conv.back_count}, leftover {leftover}; cross-canvas {len(conv.cross)}")
    for kind, msgs in REPORT.items():
        print(kind, len(msgs))
    if out_dir is None:
        return
    (out_dir / "legacy_report.json").write_text(json.dumps({"report": REPORT, "cross": conv.cross, "leftover": {f"{k[0]} | {k[1]}": v for k, v in conv.links.items() if v}}, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
