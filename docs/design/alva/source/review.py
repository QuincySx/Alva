# INPUT: Saved SmallPen packages, installed CLI, explicit review combinations
# OUTPUT: Native PNG/spec/wireframe exports, validation reports and a revision-bound index
# POS: Read-only design verification; does not change canonical packages or production code.
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
import json
import subprocess

from author import FOUNDATION, PRODUCT, ROOT
from flows import OVERLAYS, PAGES

EXPORTS = ROOT / "exports" / "archive"
REPORTS = EXPORTS / "checks"
COMBINATIONS = {
    "light-desktop": ["Appearance/Light", "Density/Comfortable", "Viewport/Desktop"],
    "light-narrow": ["Appearance/Light", "Density/Comfortable", "Viewport/Narrow"],
    "dark-desktop": ["Appearance/Dark", "Density/Comfortable", "Viewport/Desktop"],
    "dark-narrow": ["Appearance/Dark", "Density/Comfortable", "Viewport/Narrow"],
    "light-compact": ["Appearance/Light", "Density/Compact", "Viewport/Desktop"],
    "dark-narrow-compact": ["Appearance/Dark", "Density/Compact", "Viewport/Narrow"],
}


def call(args, output=None):
    flags = ["--json", "--full", "--stdout"] if args[0] == "validate" else ["--json"]
    p = subprocess.run(
        ["smallpen", *map(str, args), *flags], capture_output=True, text=True
    )
    if output:
        output.write_text(p.stdout)
        output.with_suffix(".stderr.txt").write_text(p.stderr)
    try:
        result = json.loads(p.stdout)
    except ValueError:
        raise RuntimeError(p.stdout[-2000:] + p.stderr[-2000:])
    if p.returncode or "error" in result:
        raise RuntimeError(json.dumps(result, ensure_ascii=False))
    if result.get("resultFile"):
        raise RuntimeError(
            "Expected an expanded result, not an expiring temporary resultFile"
        )
    return result


def theme_args(combination):
    return [item for theme in COMBINATIONS[combination] for item in ["--theme", theme]]


def validate(job):
    package, combination = job
    filename = (
        ("product" if package == PRODUCT else "foundation")
        + "--"
        + combination
        + ".json"
    )
    args = [
        "validate",
        package,
        *theme_args(combination),
        "--limit",
        "100",
        "--target-timeout",
        "30000",
    ]
    j = call(args, REPORTS / filename)
    return {
        "package": package.name,
        "combination": combination,
        "report": str((REPORTS / filename).relative_to(ROOT)),
        "revision": j.get("revision"),
        "foundationRevision": j.get("foundationRevision"),
        "status": j.get("status"),
        "dataStatus": j.get("dataStatus"),
        "visualStatus": j.get("visualStatus"),
        "issueCount": j.get("issueCount"),
        "coverage": j.get("coverage"),
        "issues": j.get("issues", []),
    }


def export_page(job):
    page, combination = job
    slug = page.lower().replace(" / ", "-").replace(" ", "-") + "--" + combination
    output = EXPORTS / slug
    j = call(
        [
            "export",
            PRODUCT,
            "--page",
            page,
            *theme_args(combination),
            "--format",
            "png",
            "--evidence",
            "--output",
            output,
        ]
    )
    data = json.loads(Path(j["evidencePath"]).read_text())
    return {
        "page": page,
        "combination": combination,
        "revision": j["revision"],
        "foundationRevision": j.get("foundationRevision"),
        "themes": j["selection"]["themes"],
        "layoutHash": j["layout"]["hash"],
        "renderHash": j["renderHash"],
        "diagnostics": j.get("diagnostics", []),
        "issueCount": j.get("issueCount"),
        "coverage": j.get("coverage"),
        "width": j["width"],
        "height": j["height"],
        "png": str(Path(j["imagePath"]).relative_to(ROOT)),
        "spec": str(Path(j["specPath"]).relative_to(ROOT)),
        "wireframe": str(Path(j["wireframePath"]).relative_to(ROOT)),
        "evidence": str(Path(j["evidencePath"]).relative_to(ROOT)),
        "issueCodes": sorted(set(i["code"] for i in data.get("issues", []))),
    }


def main():
    EXPORTS.mkdir(exist_ok=True)
    REPORTS.mkdir(exist_ok=True)
    packages = {
        "product": call(["project", "show", PRODUCT]),
        "foundation": call(["project", "show", FOUNDATION]),
    }
    all_pages = list(PAGES.values()) + ["Overlay / " + name for name in OVERLAYS]
    jobs = [(PRODUCT, combination) for combination in COMBINATIONS]
    jobs += [
        (FOUNDATION, combination)
        for combination in [
            "light-desktop",
            "dark-desktop",
            "light-compact",
            "dark-narrow-compact",
        ]
    ]
    validations = []
    with ThreadPoolExecutor(max_workers=2) as pool:
        for result in pool.map(validate, jobs):
            validations.append(result)
            print(
                "checked",
                result["package"],
                result["combination"],
                result["coverage"].get("completedTargetCount"),
                "targets, issues",
                result["issueCount"],
                flush=True,
            )
    exports = []
    jobs = [
        (page, combination)
        for combination in [
            "light-desktop",
            "light-narrow",
            "dark-desktop",
            "dark-narrow",
        ]
        for page in all_pages
    ]
    with ThreadPoolExecutor(max_workers=2) as pool:
        for future in as_completed([pool.submit(export_page, job) for job in jobs]):
            result = future.result()
            exports.append(result)
            if len(exports) % 10 == 0:
                print("exported", len(exports), "/", len(jobs), flush=True)
    design_system = []
    for combination in ["light-desktop", "dark-desktop"]:
        j = call(
            [
                "export",
                FOUNDATION,
                "--design-system",
                *theme_args(combination),
                "--format",
                "png",
                "--evidence",
                "--scale",
                "1",
                "--output",
                EXPORTS / ("design-system--" + combination),
            ]
        )
        design_system.append(
            {
                **{
                    k: j.get(k)
                    for k in [
                        "revision",
                        "issueCount",
                        "width",
                        "height",
                        "diagnostics",
                        "coverage",
                        "layout",
                    ]
                },
                "combination": combination,
                "png": str(Path(j["imagePath"]).relative_to(ROOT)),
                "evidence": str(Path(j["evidencePath"]).relative_to(ROOT)),
            }
        )
    after = {
        "product": call(["project", "show", PRODUCT]),
        "foundation": call(["project", "show", FOUNDATION]),
    }
    unchanged = all(packages[k]["revision"] == after[k]["revision"] for k in packages)
    if not unchanged:
        raise RuntimeError(
            "Packages changed during read-only verification; inspect changes and rerun affected checks"
        )
    previous = ROOT / "review.json"
    prior = json.loads(previous.read_text()) if previous.exists() else {}
    same_revisions = all(
        prior.get("packages", {}).get(k, {}).get("revision") == v["revision"]
        for k, v in packages.items()
    )
    review = {
        "date": "2026-10-21",
        "cli": call(["version"]),
        "packages": packages,
        "readOnlyRevisionCheck": unchanged,
        "validations": validations,
        "exports": sorted(exports, key=lambda e: (e["page"], e["combination"])),
        "designSystem": design_system,
        "manualVisualReview": (
            prior.get("manualVisualReview", []) if same_revisions else []
        ),
        "appOpening": (
            prior.get("appOpening", {"status": "pending"})
            if same_revisions
            else {
                "status": "pending",
                "reason": "Package revision changed; prior App observations do not establish this revision",
            }
        ),
        "limits": [
            "Prototype controls do not save real data or invoke model/tool/permission APIs.",
            "SmallPen overlay links do not verify production focus/scroll/drag/multi-window behavior.",
            "Automated checks cannot establish visual intent or full App acceptance.",
        ],
    }
    previous.write_text(json.dumps(review, ensure_ascii=False, indent=2) + "\n")
    print(
        "Wrote",
        previous,
        "exports",
        len(exports),
        "validation targets",
        sum(v["coverage"].get("completedTargetCount", 0) for v in validations),
        flush=True,
    )


if __name__ == "__main__":
    main()
