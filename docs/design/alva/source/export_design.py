# INPUT: docs/design/alva/alva.smallpen and the smallpen CLI (`smallpen export --component X --format png`).
# OUTPUT: <dir>/<Component>.png for every component (all its variants on one sheet), the sheets the Alva Kit gallery shows as "设计稿" when ALVA_KIT_DESIGN_DIR=<dir>.
# POS: Review helper, read-only on the package; the PNGs are local files and are never committed. Usage: python3 -I export_design.py DIR [Component ...]
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1] / "alva.smallpen"


def run(*args):
    exe = os.environ.get("SMALLPEN") or shutil.which("smallpen")
    if not exe:
        sys.exit("smallpen CLI not found on PATH (set SMALLPEN)")
    done = subprocess.run([exe, *args], capture_output=True, text=True)
    if done.returncode:
        sys.exit(f"smallpen {' '.join(args)} failed:\n{done.stdout}{done.stderr}")
    return done.stdout


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    text = json.loads(run("view", str(PACKAGE), "--components", "--as", "text", "--limit", "100", "--stdout"))["text"]
    names = [line.split(" · ")[0] for line in text.splitlines()]
    wanted = sys.argv[2:] or names
    for name in wanted:
        if name not in names:
            sys.exit("no such component: " + name)
        run("export", str(PACKAGE), "--component", name, "--format", "png", "--output", str(out / name))
        print(out / (name + ".png"))


if __name__ == "__main__":
    main()
