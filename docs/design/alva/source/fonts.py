# INPUT: Original OFL-licensed Noto Sans SC TTF and the prototype source text
# OUTPUT: Renamed 400/600 TTF/WOFF font subsets and a provenance manifest
# POS: Offline design font preparation; fonttools is an authoring-only dependency.
import hashlib
import json
from pathlib import Path
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from extend_fonts import prepare_ui_fonts

root = Path(__file__).resolve().parents[1]
source = Path(sys.argv[1])
out = root / "fonts"
out.mkdir(exist_ok=True)
inputs = list((root / "source").glob("*.py")) + [root / "DESIGN.md"]
chars = set("".join(p.read_text() for p in inputs))
chars.update(chr(x) for x in range(32, 127))
manifest = {
    "source": "https://github.com/google/fonts/tree/main/ofl/notosanssc",
    "sourceSHA256": hashlib.sha256(source.read_bytes()).hexdigest(),
    "family": "Alva Prototype Sans SC",
    "derivation": "Noto Sans SC variable font instantiated at 400 and 600, subset to the design text, renamed. SIL OFL 1.1.",
    "files": [],
}
for weight, style in [(400, "Regular"), (600, "SemiBold")]:
    f = instantiateVariableFont(TTFont(source), {"wght": weight}, inplace=True)
    options = subset.Options()
    options.name_IDs = ["*"]
    options.name_legacy = True
    options.name_languages = ["*"]
    options.layout_features = ["*"]
    sub = subset.Subsetter(options=options)
    sub.populate(unicodes=[ord(c) for c in chars])
    sub.subset(f)
    names = {
        1: "Alva Prototype Sans SC",
        2: style,
        3: f"Alva Prototype Sans SC {style} 1",
        4: f"Alva Prototype Sans SC {style}",
        6: f"AlvaPrototypeSansSC-{style}",
        16: "Alva Prototype Sans SC",
        17: style,
    }
    for name in f["name"].names:
        if name.nameID in names:
            name.string = names[name.nameID].encode(
                name.getEncoding(), errors="replace"
            )
    target = out / f"AlvaPrototypeSansSC-{style}.ttf"
    f.save(target)
    manifest["files"].append(
        {
            "file": target.name,
            "weight": weight,
            "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
            "bytes": target.stat().st_size,
        }
    )
    f.recalcTimestamp = False
    f.flavor = "woff"
    target = out / f"AlvaPrototypeSansSC-{style}.woff"
    f.save(target)
    manifest["files"].append(
        {
            "file": target.name,
            "weight": weight,
            "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
            "bytes": target.stat().st_size,
        }
    )
(out / "NOTICE.json").write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n"
)
print(json.dumps(manifest, ensure_ascii=False))
prepare_ui_fonts(out)
