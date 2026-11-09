# INPUT: design_system.token_definitions() and design_system.definitions() (the Python component model the Alva Kit follows) plus the font files in docs/design/alva/fonts.
# OUTPUT: docs/design/alva/alva-next.smallpen/ in the SmallPen HTML format: tokens.css, components/<Name>.html, assets/fonts, index.html, and source/next/token_names.json (CSS variable <-> Kit token table).
# POS: Authoring entry for the new-format design package; run it, then `smallpen check docs/design/alva/alva-next.smallpen --render`. It only writes inside alva-next.smallpen and token_names.json, never touches the legacy JSON packages.
import json
import re
import shutil
import sys
from collections import OrderedDict
from copy import deepcopy
from itertools import product
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
SOURCE = HERE.parent
sys.path.insert(0, str(SOURCE))

import design_system as ds  # noqa: E402

ALVA = SOURCE.parent
PKG = ALVA / "alva-next.smallpen"
FONTS = ALVA / "fonts"
NAMES = HERE / "token_names.json"

# ---------------------------------------------------------------- token names


def kebab(text):
    return re.sub(r"(?<!^)(?=[A-Z])", "-", text).lower()


def css_name(name, kind):
    """Kit token name + type -> SmallPen CSS variable name (without the leading --)."""
    parts = name.split(".")
    if kind == "color":
        return "color-" + kebab(parts[1])
    if kind == "font-family":
        return "font-" + kebab(parts[1])
    if kind == "font-size":
        return "text-" + kebab(parts[2])
    if kind == "font-weight":
        return "font-weight-" + kebab(parts[2])
    if kind == "number":  # SmallPen has no number type: carried as a string Token
        return "string-" + "-".join(kebab(p) for p in parts)
    if kind == "spacing":
        return "spacing-" + parts[1]
    if kind == "border-radius":
        return "radius-" + kebab(parts[1])
    if kind == "stroke-width":
        return "border-" + kebab(parts[1])
    if kind == "sizing":
        return "size-" + "-".join(kebab(p) for p in (parts[1:] if parts[0] == "size" else parts))
    if kind == "boolean":
        return "flag-" + "-".join(kebab(p) for p in (parts[1:] if parts[0] == "view" else parts))
    if kind == "shadow":
        return "shadow-" + kebab(parts[1])
    raise ValueError((name, kind))


TOKENS = ds.token_definitions()
TOK = {t["name"].removeprefix("ui."): t for t in TOKENS}
CSS = {n: css_name(n, t["type"]) for n, t in TOK.items()}
assert len(set(CSS.values())) == len(CSS), "CSS names collide"


def suffix(ref):
    """'{ui.control.height}' -> css variable suffix for class names ('control-height')."""
    name = ref[4:-1]
    css = CSS[name]
    for prefix in ("color-", "size-", "spacing-", "radius-", "border-", "shadow-", "text-", "font-weight-", "font-", "flag-"):
        if css.startswith(prefix):
            return css[len(prefix):]
    return css


def value_css(token, value):
    kind = token["type"]
    if kind == "color":
        return value
    if kind in ("font-size", "spacing", "border-radius", "stroke-width", "sizing"):
        return f"{value}px"
    if kind in ("font-weight",):
        return str(value)
    if kind == "font-family":
        return f'"{value}"'
    if kind == "number":
        return f'"{value}"'
    if kind == "boolean":
        return "true" if value else "false"
    if kind == "shadow":
        return f"{value['offsetX']}px {value['offsetY']}px {value['blur']}px {value['spread']}px {value['color']}"
    raise ValueError(kind)


# ------------------------------------------------------------- type composites
TYPES = OrderedDict()


def type_class(font):
    """font = (family ref, size ref, weight ref|int, line ref|number) -> class name; registers the composite Token."""
    family, size, weight, line = font
    fam = TOK[family[4:-1].removeprefix("ui.")] if False else None  # placeholder for clarity
    fam_name = family[4:-1]
    size_name = size[4:-1]
    weight_value = TOK[weight[4:-1]]["value"] if isinstance(weight, str) else weight
    line_value = TOK[line[4:-1]]["value"] if isinstance(line, str) else line
    size_px = TOK[size_name]["value"]
    tag = kebab(size_name.split(".")[-1])
    name = "type-" + tag
    if fam_name == "font.mono":
        name = "type-mono-" + tag
    elif fam_name == "font.icons":
        name = "type-" + tag
    if weight_value != 400:
        name += "-strong"
    if isinstance(line, str):
        if line_value != 1.5:
            name += "-" + line[4:-1].split(".")[-1]
    elif fam_name != "font.icons":
        name += f"-lh{num(line_value)}"
    value = f'{weight_value} {size_px}px/{line_value} "{TOK[fam_name]["value"]}"'
    if name in TYPES:
        assert TYPES[name] == value, (name, value, TYPES[name])
    TYPES[name] = value
    return name


# ------------------------------------------------------------ class generation
SCALE = {4: "1", 8: "2", 12: "3", 16: "4", 24: "6", 32: "8"}
RADIUS = {4: "small", 8: "ctrl", 16: "card", 99: "pill"}


def num(v):
    return str(int(v)) if float(v) == int(v) else str(v)


def spacing(v):
    if isinstance(v, str):
        return suffix(v)
    return SCALE.get(v) or f"[{num(v)}px]"


def size_class(axis, v):
    if v == "fill":
        return f"{axis}-full"
    if v == "hug":
        return f"{axis}-fit"
    if isinstance(v, str):
        return f"{axis}-{suffix(v)}"
    return f"{axis}-[{num(v)}px]"


def sides(v):
    if isinstance(v, list):
        if len(v) == 2:
            return (v[0], v[1], v[0], v[1])
        return tuple(v)
    return (v, v, v, v)


def spaced(prefix, v, keep_zero):
    t, r, b, l = sides(v)
    def one(axis, x):
        return [f"{prefix}{axis}-{spacing(x)}"] if (keep_zero or x != 0) else []
    if t == r == b == l:
        return one("", t)
    out = one("y", t) if t == b else one("t", t) + one("b", b)
    out += one("x", l) if l == r else one("l", l) + one("r", r)
    return out


def stroke_classes(v):
    if isinstance(v, dict):
        color, width = v["color"], v["width"]
    else:
        color, width = v, 1
    cc = "border-" + suffix(color)
    if isinstance(width, list):
        names = ["t", "r", "b", "l"]
        return [f"border-{n}" for n, w in zip(names, width) if w] + [cc, "border-inside"]
    if isinstance(width, str):
        return ["border-" + suffix(width), cc, "border-inside"]
    return ["border", cc, "border-inside"]


def radius_class(v):
    if isinstance(v, str):
        return "rounded-" + suffix(v)
    return "rounded-" + RADIUS[v] if v in RADIUS else f"rounded-[{num(v)}px]"


OPACITY = {0: "none", 0.6: "disabled", 0.96: "pressed", 0.98: "hover"}  # derived opacity Tokens, not part of the Kit table


def opacity_class(v):
    return f"opacity-{OPACITY[v]}" if v in OPACITY else f"opacity-[{v}]"


def classes_for(attrs, keys, keep_zero=False):
    """Class list for the given attribute keys of one element."""
    out = []
    for k in keys:
        if k not in attrs:
            continue
        v = attrs[k]
        if k == "layout":
            out += ["flex", "flex-row" if v == "row" else "flex-col"]
        elif k == "width":
            out.append(size_class("w", v))
        elif k == "height":
            out.append(size_class("h", v))
        elif k == "align":
            out.append("items-" + v)
        elif k == "justify":
            out.append("justify-" + v)
        elif k == "gap":
            out.append("gap-" + spacing(v))
        elif k == "padding":
            out += spaced("p", v, keep_zero)
        elif k == "margin":
            out += spaced("m", v, keep_zero)
        elif k == "fill":
            out.append("bg-" + suffix(v))
        elif k == "stroke":
            out += stroke_classes(v)
        elif k == "radius":
            out.append(radius_class(v))
        elif k == "shadow":
            out.append("shadow-" + suffix(v))
        elif k == "opacity":
            out.append(opacity_class(v))
        elif k == "visible":
            if v is False:
                out.append("hidden")
            elif v is True:
                out.append("shown")
        elif k == "color":
            out.append("text-" + suffix(v))
        elif k == "textAlign":
            out.append("text-" + v)
        elif k == "font":
            out.append(type_class(v))
        elif k == "flip":
            if v:
                out.append("flip-x")
    return out


BOX_KEYS = ["layout", "width", "height", "align", "justify", "gap", "padding", "margin", "fill", "stroke", "radius", "shadow", "opacity", "visible"]
TEXT_KEYS = ["width", "height", "margin", "opacity", "visible", "color", "textAlign", "font", "flip"]
GROUPS = {  # attribute key -> the class-producing keys it controls (a property replaces its whole class group)
}

# ------------------------------------------------------------------ model
DEFS = {d["name"]: d for d in ds.definitions()}


def shell_deltas():
    """Two edits of the saved package (native_shell_refresh.py) that design_system.py never received but the Kit mirrors:
    the PanelLeft symbol (the Panel glyph mirrored) and PageHeader.Navigation (Expanded | Collapsed, toggle at the start)."""
    icon = DEFS["Alva Icon"]
    panel = next(r for r in icon["variants"] if r["when"] == {"Name": "Panel"})
    icon["properties"]["Name"].append("PanelLeft")
    icon["variants"].append({"when": {"Name": "PanelLeft"}, "set": {"Glyph.text": panel["set"]["Glyph.text"], "Glyph.flip": True}})
    header = DEFS["Alva PageHeader"]
    toggle = ds.icon_button("SidebarToggle", "PanelLeft")
    toggle["visible"] = False
    header["base"]["children"].insert(0, toggle)
    header["properties"] = {"Navigation": ["Expanded", "Collapsed"]}
    header["variants"] = [{"when": {"Navigation": "Collapsed"}, "set": {"SidebarToggle.visible": True}}]


shell_deltas()


def kind_of(node):
    if node.get("use"):
        return "use"
    return "text" if "text" in node and "children" not in node else "box"


def find(node, name):
    """Descendant chain [child ... target] by name, not crossing component copies."""
    for child in node.get("children", []):
        if child["name"] == name:
            return [child]
        if child.get("use"):
            continue
        sub = find(child, name)
        if sub:
            return [child, *sub]
    return None


def canon(defname, parts):
    """Canonical chain through a component definition: [(name, kind, comp, node)]."""
    out = []
    node = DEFS[defname]["base"]
    for part in parts:
        chain = find(node, part)
        if not chain:
            raise KeyError((defname, parts, part))
        for n in chain:
            out.append((n["name"], kind_of(n), n.get("use"), n))
        node = chain[-1]
        if node.get("use"):
            node = DEFS[node["use"]]["base"]
    return out


NODES = {}  # id(node) -> def node, for override font/text base lookups


def base_font(node):
    if "fontFamily" not in node:
        return None
    return (node["fontFamily"], node["fontSize"], node["fontWeight"], node["lineHeight"])


def new_ov():
    return {"attrs": {}, "kids": OrderedDict()}


def ov_set(ov, defname, parts, field, value):
    chain = canon(defname, parts) if parts else []
    cur = ov
    for name, kind, comp, node in chain:
        if name not in cur["kids"]:
            e = new_ov()
            e.update(kind=kind, comp=comp, nid=id(node))
            NODES[id(node)] = node
            cur["kids"][name] = e
        cur = cur["kids"][name]
    set_attr(cur, field, value, chain[-1][3] if chain else None)


def set_attr(holder, field, value, node):
    attrs = holder["attrs"]
    if field == "props":
        base = dict(node.get("props") or {}) if node is not None else {}
        attrs["props"] = {**base, **attrs.get("props", {}), **value}
    else:
        attrs[field] = value


class Comp:
    """Resolved state of one component definition for one combination."""

    def __init__(self, definition):
        self.d = definition
        self.base = definition["base"]
        self.paths = OrderedDict()  # path tuple -> node
        self.kinds = {}
        self.walk(self.base, ())
        self.state0 = {}
        for path, node in self.paths.items():
            self.state0[path] = self.initial(path, node)
        self.pristine = deepcopy(self.state0)  # rules apply to this; state0 later becomes the first combination (the shared element)

    def walk(self, node, path):
        self.paths[path] = node
        for child in node.get("children", []):
            self.walk(child, (*path, child["name"]))

    def initial(self, path, node):
        a = {}
        kind = kind_of(node) if path else ("box")
        for k in ("layout", "width", "height", "align", "justify", "gap", "padding", "margin", "fill", "stroke", "radius", "shadow", "opacity"):
            if k in node:
                a[k] = node[k]
        a["visible"] = node.get("visible", True)
        if kind == "box" and not node.get("use"):
            a.setdefault("layout", "column")
            a.setdefault("align", "start")
            a.setdefault("justify", "start")
            a.setdefault("height", "hug")
        if kind == "text":
            a["text"] = node["text"]
            a["color"] = node["color"]
            a["font"] = base_font(node)
            if "textAlign" in node:
                a["textAlign"] = node["textAlign"]
            a.setdefault("height", "hug")
        if node.get("use"):
            a["props"] = dict(node.get("props") or {})
            ov = new_ov()
            defname = node["use"]
            for key, value in (node.get("text") or {}).items():
                parts = key.split(".")
                self.apply_into(ov, defname, parts, "text", value)
            for key, value in {**(node.get("set") or {}), **(node.get("_nestedSets") or {})}.items():
                parts = key.split(".")
                if len(parts) == 1:
                    a[parts[0]] = value  # copy-level override (fill)
                else:
                    self.apply_into(ov, defname, parts[:-1], parts[-1], value)
            a["ov"] = ov
            a.setdefault("height", "hug")
        return a

    def apply_into(self, ov, defname, parts, field, value):
        """Apply a set into a copy's override tree, or onto the copy itself when parts is empty."""
        if parts:
            ov_set(ov, defname, parts, field, value)
        else:
            raise ValueError("copy-level set must be handled by caller")

    def resolve_own(self, parts):
        """('own', path) | ('ov', copy path, remaining parts)."""
        node = self.base
        path = []
        for i, part in enumerate(parts):
            chain = find(node, part)
            if not chain:
                raise KeyError((self.d["name"], parts, part))
            path += [n["name"] for n in chain]
            node = chain[-1]
            if node.get("use") and i < len(parts) - 1:
                return ("ov", tuple(path), parts[i + 1:])
        return ("own", tuple(path), None)

    def combos(self):
        axes = self.d["properties"]
        names = list(axes)
        for values in product(*[axes[n] for n in names]):
            yield dict(zip(names, values))

    def state_for(self, combo):
        st = deepcopy(self.pristine)
        for rule in self.d["variants"]:
            if all(combo.get(k) == v for k, v in rule["when"].items()):
                for key, value in rule["set"].items():
                    parts = key.split(".")
                    self.apply_rule(st, parts[:-1], parts[-1], value)
        return st

    def apply_rule(self, st, parts, field, value):
        kind, path, rest = self.resolve_own(parts)
        if kind == "ov":
            attrs = st[path]
            if rest:
                ov_set(attrs["ov"], self.paths[path]["use"], rest, field, value)
                return
            raise ValueError("unreachable")
        attrs = st[path]
        node = self.paths[path]
        if field == "props":
            attrs["props"] = {**attrs.get("props", {}), **value}
        elif field == "fontWeight":
            f = list(attrs["font"]); f[2] = value; attrs["font"] = tuple(f)
        elif field == "fontSize":
            f = list(attrs["font"]); f[1] = value; attrs["font"] = tuple(f)
        elif field == "visible":
            attrs["visible"] = value
        else:
            attrs[field] = value


# ------------------------------------------------------------------ emission
def esc(text):
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def text_html(text):
    if text == "":
        return "&#8203;"  # SmallPen refuses an empty <p>; a zero-width space keeps the slot
    return "<br>".join(esc(line) for line in text.split("\n"))


def props_attr(props, domain=None):
    if not props:
        return ""
    return ' props="' + "; ".join(f"{k}: {v}" for k, v in props.items()) + '"'


def vis_attr(v):
    return f' data-visible="{suffix(v)}"' if isinstance(v, str) else ""


def class_attr(classes):
    return f' class="{" ".join(classes)}"' if classes else ""


def ov_classes(e):
    a = e["attrs"]
    node = NODES.get(e.get("nid"))
    out = []
    if e["kind"] == "text":
        font = base_font(node)
        f = list(font)
        changed = False
        for key, idx in (("fontFamily", 0), ("fontSize", 1), ("fontWeight", 2)):
            if key in a:
                f[idx] = a[key]; changed = True
        if changed:
            out.append(type_class(tuple(f)))
        if "color" in a:
            out.append("text-" + suffix(a["color"]))
    for k in ("fill", "stroke", "radius", "opacity", "padding", "width", "height"):
        if k in a:
            out += classes_for(a, [k], keep_zero=True)
    v = a.get("visible")
    if v is False:
        out.append("hidden")
    elif v is True:
        out.append("shown")
    return out


def emit_ov(kids, indent):
    pad = "  " * indent
    lines = []
    for name, e in kids.items():
        a = e["attrs"]
        cls = class_attr(ov_classes(e))
        visattr = vis_attr(a.get("visible")) if isinstance(a.get("visible"), str) else ""
        if isinstance(a.get("visible"), str):
            cls = class_attr([c for c in ov_classes(e) if c not in ("hidden", "shown")])
        if e["kind"] == "text":
            node = NODES[e["nid"]]
            hidden = a.get("visible") is False
            # no text override: leave the copy's own text alone (an Icon's glyph comes from its Name variant)
            content = "" if hidden or "text" not in a else text_html(a["text"])
            lines.append(f'{pad}<p data-name="{name}"{visattr}{cls}>{content}</p>')
        elif e["kind"] == "use":
            props = props_attr(a.get("props"))
            inner = emit_ov(e["kids"], indent + 1)
            if inner:
                lines += [f'{pad}<sp-use data-name="{name}"{props}{visattr}{cls}>', *inner, f"{pad}</sp-use>"]
            else:
                lines.append(f'{pad}<sp-use data-name="{name}"{props}{visattr}{cls}></sp-use>')
        else:
            inner = emit_ov(e["kids"], indent + 1)
            lines += [f'{pad}<div data-name="{name}"{visattr}{cls}>', *inner, f"{pad}</div>"]
    return lines


def own_keys(node):
    kind = kind_of(node)
    return TEXT_KEYS if kind == "text" else BOX_KEYS


def emit_node(path, comp, indent, state):
    node = comp.paths[path]
    a = dict(state[path])
    if path:  # a child cannot fill an axis its parent hugs
        parent = state[path[:-1]]
        for axis in ("width", "height"):
            if a.get(axis) == "fill" and parent.get(axis) == "hug":
                a[axis] = "hug"
    pad = "  " * indent
    name = node["name"]
    kind = kind_of(node)
    visattr = vis_attr(a.get("visible"))
    a2 = dict(a)
    if isinstance(a2.get("visible"), str) or a2.get("visible") is True:
        a2.pop("visible")
    if kind == "text":
        cls = classes_for(a2, TEXT_KEYS)
        if "\n" in a["text"]:
            cls.append("whitespace-pre-wrap")
        return [f'{pad}<p data-name="{name}"{visattr}{class_attr(cls)}>{text_html(a["text"])}</p>']
    if kind == "use":
        cls = classes_for(a2, ["width", "height", "margin", "opacity", "visible"])
        inner = emit_ov(a["ov"]["kids"], indent + 1)
        props = props_attr(a.get("props"))
        comp_name = node["use"].removeprefix("Alva ")
        open_tag = f'{pad}<sp-use component="{comp_name}" data-name="{name}"{props}{visattr}{class_attr(cls)}'
        if inner:
            return [open_tag + ">", *inner, f"{pad}</sp-use>"]
        return [open_tag + "></sp-use>"]
    cls = classes_for(a2, BOX_KEYS)
    lines = [f'{pad}<div data-name="{name}"{visattr}{class_attr(cls)}>']
    for child in node.get("children", []):
        lines += emit_node((*path, child["name"]), comp, indent + 1, state)
    lines.append(f"{pad}</div>")
    return lines


def group_keys(node):
    kind = kind_of(node)
    if kind == "text":
        return ["opacity", "visible", "color", "textAlign", "font", "width", "height", "margin", "flip"]
    if kind == "use":
        return ["width", "height", "margin", "opacity", "visible"]
    return BOX_KEYS


def hidden_in(state, path):
    return any(state[path[:i]].get("visible") is False for i in range(1, len(path) + 1))


def diff_state(comp, state):
    """path -> changed attrs (dict) for every own node that differs from the base state."""
    out = {}
    for path, a in state.items():
        b = comp.state0[path]
        if path and hidden_in(state, path):
            # a hidden part shows nowhere: only its visibility may differ
            if a.get("visible") != b.get("visible") and a.get("visible") is False and not hidden_in(state, path[:-1]):
                out[path] = {"visible": False}
            continue
        changed = {}
        for k in set(a) | set(b):
            if a.get(k) != b.get(k):
                changed[k] = a.get(k)
        if changed:
            out[path] = changed
    return out


def key_of(diff):
    return json.dumps({'/'.join(p): v for p, v in diff.items()}, sort_keys=True, ensure_ascii=False, default=str)


def emit_diff(comp, state, diff, indent):
    """sp-variant body: parts changed vs the shared element, nested as in the component."""
    need = set()
    for path in diff:
        for i in range(1, len(path) + 1):
            need.add(path[:i])

    def part(path, indent):
        pad = "  " * indent
        node = comp.paths[path]
        kind = kind_of(node)
        a = state[path]
        changed = diff.get(path, {})
        name = node["name"]
        keys = [k for k in group_keys(node) if k in changed]
        cls = classes_for({k: a[k] for k in keys if k in a and a[k] is not None}, keys, keep_zero=True)
        vis = ""
        if isinstance(a.get("visible"), str):
            cls = [c for c in cls if c not in ("hidden", "shown")]
        if kind == "text":
            hidden = a.get("visible") is False
            content = "" if hidden else text_html(a["text"])
            return [f'{pad}<p data-name="{name}"{vis}{class_attr(cls)}>{content}</p>']
        if kind == "use":
            props = props_attr(a.get("props"))
            inner = emit_ov(a["ov"]["kids"], indent + 1)
            tag = f'{pad}<sp-use data-name="{name}"{props}{vis}{class_attr(cls)}'
            return [tag + ">", *inner, f"{pad}</sp-use>"] if inner else [tag + "></sp-use>"]
        out = [f'{pad}<div data-name="{name}"{class_attr(cls)}>']
        for child in node.get("children", []):
            cp = (*path, child["name"])
            if cp in need:
                out += part(cp, indent + 1)
        out.append(f"{pad}</div>")
        return out

    body = []
    for child in comp.base.get("children", []):
        cp = (child["name"],)
        if cp in need:
            body += part(cp, indent)
    root_cls = []
    if () in diff:
        keys = [k for k in group_keys(comp.paths[()]) if k in diff[()]]
        root_cls = classes_for({k: state[()][k] for k in keys if k in state[()]}, keys, keep_zero=True)
    return root_cls, body


def reduce_when(combo_list, key_by_combo, axes):
    """Minimal `when` for each group of combos sharing one diff: drop axes that do not matter."""
    rules = []
    by_key = OrderedDict()
    for combo, key in key_by_combo:
        by_key.setdefault(key, []).append(combo)
    allcombos = [c for c, _ in key_by_combo]
    keymap = {json.dumps(c, sort_keys=True): k for c, k in key_by_combo}
    for key, members in by_key.items():
        keep = list(axes)
        for axis in list(axes):
            trial = [a for a in keep if a != axis]
            ok = True
            for c in allcombos:
                if all(c[a] == members[0][a] for a in trial):
                    if keymap[json.dumps(c, sort_keys=True)] != key:
                        ok = False
                        break
            if ok:
                keep = trial
        seen = set()
        for m in members:
            w = tuple((a, m[a]) for a in keep)
            if w not in seen:
                seen.add(w)
                rules.append((w, key, m))
    return rules


def emit_component(definition):
    comp = Comp(definition)
    name = definition["name"].removeprefix("Alva ")
    axes = definition["properties"]
    combos = list(comp.combos())
    base_combo = combos[0]
    lines = []
    props = "; ".join(f"{k}: {' | '.join(v)}" for k, v in axes.items())
    head = f'<sp-component name="{name}"' + (f' props="{props}"' if props else "") + ">"
    lines.append(head)
    # the shared element is the first combination's resolved state
    first_state = comp.state_for(base_combo)
    lines += emit_node((), comp, 1, first_state)
    # variants: diff of every other combination against the first
    comp.state0 = first_state
    results = []
    for combo in combos:
        st = comp.state_for(combo)
        d = diff_state(comp, st)
        results.append((combo, key_of(d), d, st))
    key_by_combo = [(c, k) for c, k, _, _ in results]
    empty_key = key_of({})
    detail = {k: (d, st) for _, k, d, st in results}
    for when, key, member in reduce_when([c for c, *_ in results], [(c, k) for c, k in key_by_combo if k != empty_key] and key_by_combo, list(axes)):
        if key == empty_key:
            continue
        d, st = detail[key]
        root_cls, body = emit_diff(comp, st, d, 2)
        when_txt = "; ".join(f"{a}: {v}" for a, v in when)
        open_tag = "  <sp-variant" + (f' when="{when_txt}"' if when_txt else "") + class_attr(root_cls)
        lines += [open_tag + (">" if body else "></sp-variant>")] + ([*body, "  </sp-variant>"] if body else [])
    lines.append("</sp-component>")
    return name, "\n".join(lines) + "\n", len(combos)


# ------------------------------------------------------------------ package
def tokens_css():
    out = []
    ff = [
        ("Alva UI Sans SC", 400, "AlvaUISansSC-Regular.ttf"),
        ("Alva UI Sans SC", 600, "AlvaUISansSC-SemiBold.ttf"),
        ("JetBrains Mono", 400, "JetBrainsMono-Regular.ttf"),
        ("Alva Interface Icons", 400, "AlvaInterfaceIcons-Regular.ttf"),
    ]
    for fam, weight, file in ff:
        out.append(f'@font-face {{ font-family: "{fam}"; font-weight: {weight}; src: url(assets/fonts/{file}); }}')
    out.append(":root {")
    for n, t in TOK.items():
        out.append(f"  --{CSS[n]}: {value_css(t, t['value'])};")
    for tname, tvalue in TYPES.items():
        out.append(f"  --{tname}: {tvalue};")
    for value, tag in OPACITY.items():
        out.append(f"  --opacity-{tag}: {value};")
    out.append("}")
    dark = [(n, t["values"]["Appearance/Dark"]) for n, t in TOK.items() if "Appearance/Dark" in t.get("values", {})]
    out.append(':root[data-appearance="light"] {}')
    out.append(':root[data-appearance="dark"] {')
    for n, v in dark:
        out.append(f"  --{CSS[n]}: {value_css(TOK[n], v)};")
    out.append("}")
    narrow = [(n, t["values"]["Viewport/Narrow"]) for n, t in TOK.items() if "Viewport/Narrow" in t.get("values", {})]
    out.append(':root[data-viewport="desktop"] {}')
    out.append(':root[data-viewport="narrow"] {')
    for n, v in narrow:
        out.append(f"  --{CSS[n]}: {value_css(TOK[n], v)};")
    out.append("}")
    return "\n".join(out) + "\n"


def names_json():
    rows = [{"kit": n, "css": CSS[n], "type": t["type"]} for n, t in TOK.items()]
    derived = sorted(TYPES) + ["opacity-" + tag for tag in OPACITY.values()]
    return json.dumps({"version": 1, "tokens": rows, "derived": derived}, ensure_ascii=False, indent=1) + "\n"


INDEX = """<!doctype html>
<html data-appearance="light" data-viewport="desktop">
<head>
  <meta charset="utf-8">
  <title>Alva</title>
  <meta name="smallpen:package-id" content="pkg_6f1c0a8e2b5d4c379a1e5d0b7c3f2a41">
</head>
<body></body>
</html>
"""


FONT_FILES = ("AlvaUISansSC-Regular.ttf", "AlvaUISansSC-SemiBold.ttf", "JetBrainsMono-Regular.ttf", "AlvaInterfaceIcons-Regular.ttf")


def generate():
    """Everything the generator owns: relative path -> text (components, tokens.css, token_names.json)."""
    comps = OrderedDict()
    total = 0
    for d in DEFS.values():
        n, text, count = emit_component(d)
        comps["components/" + n + ".html"] = text
        total += count
    files = OrderedDict(comps)
    files["tokens.css"] = tokens_css()  # after the components: type Tokens are registered while they are emitted
    files["../source/next/token_names.json"] = names_json()
    return files, total


def check():
    """Exit non-zero when the package differs from what the generator would write (no CLI needed)."""
    files, _ = generate()
    problems = []
    for rel, text in files.items():
        path = PKG / rel
        if not path.exists():
            problems.append("missing " + rel)
        elif path.read_text(encoding="utf-8") != text:
            problems.append("differs " + rel)
    existing = {f"components/{p.name}" for p in (PKG / "components").glob("*.html")}
    problems += ["stale " + rel for rel in sorted(existing - set(files))]
    for file in FONT_FILES:
        font = PKG / "assets" / "fonts" / file
        if not font.exists() or font.read_bytes() != (FONTS / file).read_bytes():
            problems.append("font " + file)
    if not (PKG / "index.html").exists():
        problems.append("missing index.html")
    if problems:
        print("alva-next.smallpen is out of date with design_system.py / build_next.py; run python3 -I docs/design/alva/source/next/build_next.py\n  " + "\n  ".join(problems), file=sys.stderr)
        sys.exit(1)
    print("alva-next.smallpen matches the generator")


def main():
    if sys.argv[1:] == ["--check"]:
        return check()
    files, total = generate()
    PKG.mkdir(exist_ok=True)
    if not (PKG / "index.html").exists():
        (PKG / "index.html").write_text(INDEX, encoding="utf-8")
    (PKG / "components").mkdir(exist_ok=True)
    (PKG / "assets" / "fonts").mkdir(parents=True, exist_ok=True)
    for stale in (PKG / "components").glob("*.html"):  # a component removed from the model must not linger in the package
        if f"components/{stale.name}" not in files:
            stale.unlink()
    for file in FONT_FILES:
        shutil.copyfile(FONTS / file, PKG / "assets" / "fonts" / file)
    for rel, text in files.items():
        (PKG / rel).write_text(text, encoding="utf-8")
    print(f"{len(files) - 2} components, {total} combinations, {len(TYPES)} type tokens")


if __name__ == "__main__":
    main()
