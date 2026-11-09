# INPUT: The legacy package alva.smallpen (a read-only copy is safest), the alpha.21 SmallPen CLI that can still read it, the new smallpen CLI and alva-next.smallpen.
# OUTPUT: Per page, the visible texts the legacy and the new renderings show (from `export --format spec`, hidden nodes and descendants of hidden nodes dropped) and their differences; optionally the light-desktop PNG of both sides.
# POS: One-time safety net for the legacy conversion (see pages_from_legacy.py); remove together with the legacy packages. Usage: python3 -I compare_legacy.py OLD_CLI LEGACY_PACKAGE OUT_DIR [--png] [page name ...]
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
NEW_PKG = HERE.parents[1] / "alva-next.smallpen"


def run(*args):
    done = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    if done.returncode:
        raise RuntimeError(f"{args}: {done.stdout[:300]} {done.stderr[:300]}")


def visible_texts(spec_text):
    """Texts of visible nodes from a spec dump: TEXT nodes (`text:` lines) and `X.text=` overrides of visible copies."""
    out, stack = [], []  # stack of (indent, hidden)
    for line in spec_text.splitlines():
        m = re.match(r"^(\s*)\[\d+\] (.*)$", line)
        if m:
            indent = len(m.group(1))
            while stack and stack[-1][0] >= indent:
                stack.pop()
            hidden = " · hidden" in m.group(2) or (stack and stack[-1][1])
            stack.append((indent, bool(hidden)))
            continue
        if not stack or stack[-1][1]:
            continue
        t = re.match(r'^\s*text: "(.*)" · ', line)
        if t:
            out.append(t.group(1))
        o = re.match(r"^\s*page overrides: (.*)$", line)
        if o:
            for _, value in re.findall(r"([\w./]+)\.text=(.*?)(?=; [\w./]+\.\w+=|$)", o.group(1)):
                if value:
                    out.append(value)
    return [t for t in (re.sub(r"[\s\u200b]+", " ", t).strip() for t in out) if t]  # runs of spaces fold in the new format (as in HTML)


def layer_key(wireframe_text):
    """(depth path of names, type, hidden) of every layer in the wireframe's LAYER KEY (trailing digits of names dropped: the conversion de-duplicates siblings with a number)."""
    out, stack = [], []
    in_key = False
    for line in wireframe_text.splitlines():
        if line.startswith("LAYER KEY"):
            in_key = True
            continue
        if not in_key:
            continue
        m = re.match(r'^(\s*)([\[(])\d+[\])] (HIDDEN )?(\w+) "(.*)"$', line)
        if not m:
            continue
        depth = len(m.group(1)) // 2
        name = re.sub(r"\d+$", "", m.group(5))
        del stack[depth:]
        stack.append(name)
        out.append(("/".join(stack[1:]), m.group(4), bool(m.group(3))))
    return out


def export(cli, package, page, fmt, dest, extra=()):
    path = Path(dest)
    path.parent.mkdir(parents=True, exist_ok=True)
    final = Path(str(path) + (".png" if fmt == "png" else ".txt"))
    if final.exists():  # reuse an earlier export (delete the output directory to refresh)
        return final
    run(cli, "export", package, "--page", page, "--format", fmt, "--output", path, *extra)
    ext = ".png" if fmt == "png" else ".txt"
    final = Path(str(path) + ext)
    if path.exists() and not final.exists():  # the legacy CLI writes without an extension
        path.rename(final)
    return final


def main():
    args = [a for a in sys.argv[1:] if a != "--png"]
    png = "--png" in sys.argv
    old_cli, legacy, out = args[0], Path(args[1]), Path(args[2])
    only = args[3:]
    manifest = json.loads((legacy / "manifest.json").read_text())
    canvas = {s: c["name"] for c in manifest["canvases"] for s in c["screens"]}
    report = out / "text_diff.json"
    result = json.loads(report.read_text()) if report.exists() else {}
    for f in sorted((legacy / "screens").glob("*.json")):
        screen = json.loads(f.read_text())
        old_name = screen["name"]
        if only and old_name not in only and old_name.replace(" / ", "/") not in only:
            continue
        new_name = old_name.replace(" / ", "/")
        slug = re.sub(r"[^\w]+", "-", old_name).strip("-")
        old_spec = export(old_cli, legacy, old_name, "spec", out / "spec-old" / slug, ("--platform", "desktop"))
        new_spec = export("smallpen", NEW_PKG, new_name, "spec", out / "spec-new" / slug)
        old_wf = export(old_cli, legacy, old_name, "wireframe", out / "wf-old" / slug, ("--platform", "desktop"))
        new_wf = export("smallpen", NEW_PKG, new_name, "wireframe", out / "wf-new" / slug)
        la, lb = Counter(layer_key(old_wf.read_text())), Counter(layer_key(new_wf.read_text()))
        a = Counter(visible_texts(old_spec.read_text()))
        b = Counter(visible_texts(new_spec.read_text()))
        result[old_name] = {"canvas": canvas[screen["id"]], "old_only": sorted((a - b).elements()), "new_only": sorted((b - a).elements()), "old_count": sum(a.values()), "new_count": sum(b.values()),
                           "layers_old": sum(la.values()), "layers_new": sum(lb.values()),
                           "layers_old_only": sorted(f"{h and 'hidden ' or ''}{t} {p}" for (p, t, h) in (la - lb).elements()),
                           "layers_new_only": sorted(f"{h and 'hidden ' or ''}{t} {p}" for (p, t, h) in (lb - la).elements())}
        if png:
            export(old_cli, legacy, old_name, "png", out / "png" / f"{slug}-old", ("--platform", "desktop"))
            export("smallpen", NEW_PKG, new_name, "png", out / "png" / f"{slug}-new")
        r = result[old_name]
        print(old_name, "texts -%d +%d" % (len(r["old_only"]), len(r["new_only"])), "layers -%d +%d" % (len(r["layers_old_only"]), len(r["layers_new_only"])), flush=True)
    report.write_text(json.dumps(result, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
