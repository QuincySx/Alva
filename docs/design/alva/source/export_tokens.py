# INPUT: The SmallPen CLI (`smallpen token list`, `smallpen view --components`) reading docs/design/alva/alva.smallpen, plus source/token_names.json (CSS variable <-> Kit token table).
# OUTPUT: apps/ui/kit/theme/tokens.json and apps/ui/kit/testdata/design-components.json, byte-for-byte the format the Kit has always committed.
# POS: Offline bridge from the design package to the Go Alva Kit; Go builds never run it, tests compare its output with the committed JSON. Any name, type or catalog mismatch is an error, never a silent skip.
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
KIT = REPO / "apps" / "ui" / "kit"
PACKAGE = Path(os.environ.get("ALVA_DESIGN_PACKAGE") or HERE.parent / "alva.smallpen")
NAMES = HERE / "token_names.json"


class Mismatch(Exception):
    pass


def smallpen(*args):
    exe = os.environ.get("SMALLPEN") or shutil.which("smallpen")
    if not exe:
        raise Mismatch("smallpen CLI not found on PATH (set SMALLPEN to its path)")
    done = subprocess.run([exe, *args], capture_output=True, text=True)
    if done.returncode:
        raise Mismatch(f"smallpen {' '.join(args)} failed: {done.stdout}{done.stderr}")
    return done.stdout


def number(text, token):
    try:
        value = float(text)
    except (TypeError, ValueError):
        raise Mismatch(f"Token --{token} carries {text!r}, which is not a number") from None
    return int(value) if value == int(value) else value


def kit_value(kind, value):
    if kind == "color":
        return value.upper()
    if kind == "shadow":
        return {**value, "color": value["color"].upper()}
    return value


def tokens():
    table = json.loads(NAMES.read_text(encoding="utf-8"))
    by_css = {row["css"]: row for row in table["tokens"]}
    derived = set(table["derived"])
    listing = json.loads(smallpen("token", "list", str(PACKAGE), "--limit", "100", "--stdout"))
    if listing["page"]["hasMore"]:
        raise Mismatch("token list has more than 100 Tokens; page it")
    found = {}
    seen_derived = set()
    for item in listing["items"]:
        path, kind = item["token"]["path"], item["token"]["type"]
        if path in derived:
            seen_derived.add(path)
            continue
        row = by_css.get(path)
        if row is None:
            raise Mismatch(f"Token --{path} is in the package but not in token_names.json; add it to build_next.py and regenerate")
        want = "string" if row["type"] == "number" else row["type"]
        if kind != want:
            raise Mismatch(f"Token --{path} has type {kind}, the table says {want}")
        found[path] = item
    if seen_derived != derived:
        raise Mismatch("derived Tokens (type-*, opacity-*) differ from token_names.json: extra "
                       f"{sorted(seen_derived - derived)}, missing {sorted(derived - seen_derived)}; regenerate with build_next.py")
    missing = sorted(set(by_css) - set(found))
    if missing:
        raise Mismatch("Tokens missing from the package: " + ", ".join(missing))
    out = []
    for row in table["tokens"]:
        item = found[row["css"]]
        convert = (lambda v: number(v, row["css"])) if row["type"] == "number" else (lambda v: kit_value(row["type"], v))
        entry = {"name": row["kit"], "type": row["type"], "value": convert(item["value"])}
        values = item.get("values", {})
        if "appearance/dark" in values:
            entry["dark"] = convert(values["appearance/dark"])
        if "viewport/narrow" in values:
            entry["narrow"] = convert(values["viewport/narrow"])
        extra = set(values) - {"appearance/dark", "viewport/narrow"}
        if extra:
            raise Mismatch(f"Token --{row['css']} varies by unknown theme options: {sorted(extra)}")
        out.append(entry)
    return {"version": 1, "tokens": out}


def components():
    text = json.loads(smallpen("view", str(PACKAGE), "--components", "--as", "text", "--limit", "100", "--stdout"))["text"]
    out = []
    for line in text.splitlines():
        parts = line.split(" · ")
        name, last = parts[0], parts[-1]
        match = re.fullmatch(r"(\d+) variants?", last)
        if not match:
            raise Mismatch("unreadable component line: " + line)
        axes = []
        for part in parts[1:-1]:
            axis, values = part.split(": ", 1)
            axes.append({"name": axis, "values": values.split("|")})
        out.append({"name": name, "variants": int(match.group(1)), "axes": axes})
    return {"version": 1, "components": out}


def write(path, document):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    # Optional argument: an alternative kit root, so tests can compare without touching the repository.
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else KIT
    try:
        write(root / "theme" / "tokens.json", tokens())
        write(root / "testdata" / "design-components.json", components())
    except Mismatch as error:
        print("export_tokens: " + str(error), file=sys.stderr)
        sys.exit(1)
