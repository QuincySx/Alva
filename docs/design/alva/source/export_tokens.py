# INPUT: design_system.token_definitions() and the saved Foundation package's components/components.json (read only).
# OUTPUT: apps/ui/kit/theme/tokens.json and apps/ui/kit/testdata/design-components.json, deterministic and sorted as authored.
# POS: Offline bridge from the design source to the Go Alva Kit; Go builds never run it, tests compare its output with the committed JSON.
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import design_system  # noqa: E402  (import has no side effects; its entry is guarded)

REPO = HERE.parents[3]
KIT = REPO / "apps" / "ui" / "kit"
COMPONENTS = HERE.parent / "alva-foundation.smallpen" / "components" / "components.json"


def tokens():
    """Flatten token_definitions(): name without the ui. prefix, plus dark and narrow overrides."""
    out = []
    for token in design_system.token_definitions():
        entry = {"name": token["name"].removeprefix("ui."), "type": token["type"], "value": token["value"]}
        overrides = token.get("values", {})
        if "Appearance/Dark" in overrides:
            entry["dark"] = overrides["Appearance/Dark"]
        if "Viewport/Narrow" in overrides:
            entry["narrow"] = overrides["Viewport/Narrow"]
        out.append(entry)
    return {"version": 1, "tokens": out}


def components():
    """Formal Alva components only; the unprefixed historical sets are ignored by the kit."""
    sets = json.loads(COMPONENTS.read_text(encoding="utf-8"))["componentSets"]
    out = []
    for component in sets:
        if not component["name"].startswith("Alva "):
            continue
        out.append({
            "name": component["name"].removeprefix("Alva "),
            "variants": len(component["variants"]),
            "axes": [{"name": axis["name"], "values": list(axis["domain"])} for axis in component["axes"]],
        })
    return {"version": 1, "components": out}


def write(path, document):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    # Optional argument: an alternative kit root, so tests can compare without touching the repository.
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else KIT
    write(root / "theme" / "tokens.json", tokens())
    write(root / "testdata" / "design-components.json", components())
