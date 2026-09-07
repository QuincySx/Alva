#!/usr/bin/env python3
"""Public-surface scanner for the snapshot gate (kernel-boundary refactor, ticket 01).

Extracts the public symbol surface of a crate from its Rust source and prints
one normalized record per symbol. The output is deterministic: same source,
same records, byte for byte. `scripts/public-surface.sh` diffs this output
against the checked-in snapshot.

Record shapes (one per line, TAB-separated):

    const   <path> <normalized header>
    enum    <path>
    fn      <path> <normalized header>
    implfn  <Type>::<method> <normalized header>
    mod     <path>
    static  <path> <normalized header>
    struct  <path>
    trait   <path>
    traitfn <Trait>::<method> <normalized header>
    type    <path> <normalized header>
    use     <normalized use item>

Only plain `pub` counts — `pub(crate)` / `pub(super)` / `pub(in ..)` are not
public API. Methods of trait impls are not records (the owning trait already
carries them); inherent `impl` methods are. String literal contents are
replaced by their length (`"s12"`) so value changes still surface in the diff
while brace parsing stays safe.
"""

import re
import sys
from pathlib import Path

DECL_RE = re.compile(r"^pub(\((?:crate|super|self|in\s[^)]*)\))?\s*(?P<rest>.*)$", re.DOTALL)

ITEM_KIND_RE = re.compile(
    r"^(?:(?:async|unsafe|extern\s+\"[^\"]*\"|const)\s+)*"
    r"(?P<lead>(?:async|unsafe|const)\s+)*"
    r"(?P<kind>struct|enum|union|fn|const|static|type|mod|trait|impl|use)\b"
    r"(?P<after>.*)$",
    re.DOTALL,
)

FN_NAME_RE = re.compile(r"fn\s+([A-Za-z_][A-Za-z0-9_]*)")
IMPL_RE = re.compile(
    r"^impl\b[^{]*?(?:\s([A-Za-z_][A-Za-z0-9_:]*)\s+for\s+)?" r"\s*([A-Za-z_][A-Za-z0-9_]*)"
)


def strip_comments_and_strings(src: str) -> str:
    """Blank out comments and string/char literal contents, preserving length
    and newlines so brace scanning stays aligned with the original. String
    literal contents become `s<len>` markers."""
    out = list(src)
    i = 0
    n = len(src)
    state = None  # None | 'line' | 'block' | 'str' | ('raw', hashes)
    raw_hashes = 0
    while i < n:
        c = src[i]
        if state is None:
            if c == "/" and i + 1 < n and src[i + 1] == "/":
                state = "line"
                out[i] = " "
                out[i + 1] = " "
                i += 2
                continue
            if c == "/" and i + 1 < n and src[i + 1] == "*":
                state = "block"
                out[i] = " "
                out[i + 1] = " "
                i += 2
                continue
            if c == '"':
                state = "str"
                raw_hashes = 0
                i += 1
                continue
            if c == "r":
                m = re.match(r'r#*"', src[i:])
                if m:
                    raw_hashes = m.group(0).count("#") - 1
                    state = "raw"
                    for j in range(i, i + len(m.group(0))):
                        out[j] = " "
                    i += len(m.group(0))
                    continue
                i += 1
                continue
            if c == "'":
                close = _char_literal_close(src, i)
                if close is not None:
                    for j in range(i, close + 1):
                        out[j] = " " if src[j] != "\n" else "\n"
                    i = close + 1
                    continue
                i += 1
                continue
            i += 1
            continue
        if state == "line":
            if c == "\n":
                state = None
            else:
                out[i] = " "
            i += 1
            continue
        if state == "block":
            if c == "*" and i + 1 < n and src[i + 1] == "/":
                out[i] = " "
                out[i + 1] = " "
                state = None
                i += 2
                continue
            if c != "\n":
                out[i] = " "
            i += 1
            continue
        if state == "str":
            if c == "\\":
                out[i] = " "
                if i + 1 < n:
                    out[i + 1] = " "
                i += 2
                continue
            if c == '"':
                state = None
                i += 1
                continue
            if c != "\n":
                out[i] = " "
            i += 1
            continue
        if state == "raw":
            marker = '"' + "#" * raw_hashes
            end = src.find(marker, i)
            if end == -1:
                i = n
            else:
                seg = src[i:end]
                length = len(seg)
                repl = f"s{length}"
                for j in range(i, end):
                    out[j] = " "
                # Write the marker into the first chars of the blanked run.
                for k, ch in enumerate(repl):
                    if i + k < end and src[i + k] != "\n":
                        out[i + k] = ch
                i = end + len(marker)
                state = None
            continue
    return "".join(out)


def _char_literal_close(src: str, i: int):
    """Index of the closing quote if src[i] starts a char literal, else None."""
    j = i + 1
    if j < len(src) and src[j] == "\\":
        j += 1
        if j < len(src) and src[j] == "u" and j + 1 < len(src) and src[j + 1] == "{":
            close = src.find("}", j)
            if close == -1:
                return None
            j = close + 1
        else:
            j += 1
    if j < len(src) and src[j] == "'":
        return j
    return None


def normalize(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def find_block_end(src: str, open_brace: int) -> int:
    """Index just past the matching `}` for the `{` at open_brace."""
    depth = 0
    i = open_brace
    n = len(src)
    while i < n:
        c = src[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def find_header_end(src: str, start: int):
    """Scan from `start` to the first top-level `{` or `;`. Returns (char, index)."""
    i = start
    n = len(src)
    while i < n:
        c = src[i]
        if c == "{" or c == ";":
            return c, i
        i += 1
    return None, n


def skip_attributes(src: str, i: int) -> int:
    """Skip `#[...]` / `#![...]` attribute spans starting at i; return next index."""
    n = len(src)
    while i < n and src[i] == "#":
        j = i + 1
        if j < n and src[j] == "!":
            j += 1
        if j < n and src[j] == "[":
            depth = 0
            while j < n:
                if src[j] == "[":
                    depth += 1
                elif src[j] == "]":
                    depth -= 1
                    if depth == 0:
                        j += 1
                        break
                j += 1
            i = j
        else:
            break
    return i


def emit_fn_record(records, kind_tag, path, header_txt):
    sig = normalize(header_txt.lstrip())
    records.append((kind_tag, f"{path} {sig}"))


def scan_scope(src: str, prefix: str, public: bool, records: list) -> None:
    """Walk one brace scope, emitting records for public items."""
    i = 0
    n = len(src)
    while i < n:
        c = src[i]
        if c.isspace():
            i += 1
            continue
        if c == "}":
            i += 1
            continue
        i = skip_attributes(src, i)
        if i >= n:
            break
        while i < n and src[i].isspace():
            i += 1
        if i >= n:
            break
        c = src[i]

        header_start = i
        kind, hdr_end = find_header_end(src, i)
        if kind is None:
            break
        header = src[header_start:hdr_end]

        m = DECL_RE.match(header)
        is_pub_decl = bool(m)
        vis_pub = bool(m and not m.group(1))  # plain `pub` only
        rest = m.group("rest") if m else header.lstrip()
        rest = rest.lstrip()

        kind_m = ITEM_KIND_RE.match(rest)
        if not kind_m:
            # Unrecognized item (macro invocation, etc.) — skip its span.
            if kind == "{":
                i = find_block_end(src, hdr_end)
            else:
                i = hdr_end + 1
            continue

        kw = kind_m.group("kind")
        item_public = public and vis_pub

        if kw == "use":
            # use trees contain braces — rescan to the terminating `;`.
            semi = src.find(";", hdr_end)
            if semi == -1:
                break
            full = normalize(src[header_start:semi])
            if item_public:
                records.append(("use", full))
            i = semi + 1
            continue

        if kw == "mod":
            name_m = re.match(r"mod\s+([A-Za-z_][A-Za-z0-9_]*)", rest)
            name = name_m.group(1) if name_m else "?"
            if kind == "{":
                if item_public:
                    records.append(("mod", prefix + name))
                block_end = find_block_end(src, hdr_end)
                scan_scope(src[hdr_end + 1:block_end - 1], prefix + name + "::",
                           item_public, records)
                i = block_end
            else:
                if item_public:
                    records.append(("mod", prefix + name))
                i = hdr_end + 1
            continue

        if kw == "trait":
            name_m = re.match(r"trait\s+([A-Za-z_][A-Za-z0-9_]*)", rest)
            name = name_m.group(1) if name_m else "?"
            if item_public:
                records.append(("trait", prefix + name))
            if kind == "{":
                block_end = find_block_end(src, hdr_end)
                scan_members(src[hdr_end + 1:block_end - 1], prefix + name,
                             item_public, records, "traitfn")
                i = block_end
            else:
                i = hdr_end + 1
            continue

        if kw == "impl":
            if kind == "{":
                block_end = find_block_end(src, hdr_end)
                scan_impl(src[hdr_end + 1:block_end - 1], rest, public, records)
                i = block_end
            else:
                i = hdr_end + 1
            continue

        # struct / enum / union / fn / const / static / type
        name_m = re.match(
            r"(?:struct|enum|union|fn|const|static|type)\s+([A-Za-z_][A-Za-z0-9_]*)", rest
        )
        if not name_m:
            if kind == "{":
                i = find_block_end(src, hdr_end)
            else:
                i = hdr_end + 1
            continue
        name = name_m.group(1)
        path = prefix + name
        if item_public:
            if kw in ("struct", "enum", "union"):
                records.append((kw, path))
            else:
                records.append((kw, f"{path} {normalize(rest)}"))
        if kind == "{":
            i = find_block_end(src, hdr_end)
        else:
            i = hdr_end + 1


def scan_members(src: str, owner_path: str, public: bool, records: list, tag: str) -> None:
    """Scan a trait body for method signatures."""
    i = 0
    n = len(src)
    while i < n:
        c = src[i]
        if c.isspace() or c == "}":
            i += 1
            continue
        i = skip_attributes(src, i)
        if i >= n:
            break
        while i < n and src[i].isspace():
            i += 1
        if i >= n:
            break
        header_start = i
        kind, hdr_end = find_header_end(src, i)
        if kind is None:
            break
        header = src[header_start:hdr_end].lstrip()
        pub_kw = r"(?:pub(?:\([^)]*\))?\s+)?" if tag == "traitfn" else r"pub(?:\([^)]*\))?\s+"
        fn_m = re.match(pub_kw + r"((?:async\s+)?fn\s+[A-Za-z_][A-Za-z0-9_]*)", header)
        if fn_m and public:
            sig = normalize(header)
            records.append((tag, f"{owner_path}::{fn_m.group(1)} {sig}"))
        if kind == "{":
            i = find_block_end(src, hdr_end)
        else:
            i = hdr_end + 1


def classify_impl(header: str):
    """`impl [Trait for] Type<..>` header → (trait_name | None, type_name)."""
    rest = header[len("impl"):].lstrip()
    # Skip a leading generic parameter list with balanced angle brackets.
    if rest.startswith("<"):
        depth = 0
        for idx, ch in enumerate(rest):
            if ch == "<":
                depth += 1
            elif ch == ">":
                depth -= 1
                if depth == 0:
                    rest = rest[idx + 1:].lstrip()
                    break
        else:
            return None, None
    # Strip a trailing where clause; everything left is `Trait for Type` or `Type`.
    where_idx = rest.find(" where ")
    if where_idx != -1:
        rest = rest[:where_idx]
    for_probe = re.search(r"\s+for\s+", rest)
    if for_probe:
        trait_name = rest[:for_probe.start()].strip()
        type_name = rest[for_probe.end():].strip()
        return trait_name, type_name.split()[0] if type_name.split() else None
    parts = rest.split()
    return None, (parts[0] if parts else None)


def scan_impl(src: str, header: str, public: bool, records: list) -> None:
    trait_name, type_name = classify_impl(header)
    if type_name is None:
        return
    if trait_name:
        return  # trait impl — methods belong to the trait, not this surface
    scan_members(src, type_name, public, records, "implfn")


def module_prefix(rel_path: Path) -> str:
    """Rust module path prefix for a source file: src/lib.rs → '', src/tool/mod.rs
    → 'tool::', src/tool/types.rs → 'tool::types::'."""
    parts = list(rel_path.with_suffix("").parts)
    if parts and parts[-1] == "lib":
        parts = parts[:-1]
    elif parts and parts[-1] == "mod":
        parts = parts[:-1]
    if not parts:
        return ""
    return "::".join(parts) + "::"


def crate_records(crate_dir: Path) -> list:
    records = []
    src_dir = crate_dir / "src"
    for path in sorted(src_dir.rglob("*.rs")):
        rel = path.relative_to(src_dir)
        text = strip_comments_and_strings(path.read_text(encoding="utf-8"))
        scan_scope(text, module_prefix(rel), True, records)
    return records


def render(records: list) -> str:
    lines = [f"{kind}\t{detail}" for kind, detail in records]
    lines.sort()
    return "\n".join(lines) + "\n"


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: public-surface.py <crate-src-dir>", file=sys.stderr)
        return 2
    sys.stdout.write(render(crate_records(Path(sys.argv[1]))))
    return 0


if __name__ == "__main__":
    sys.exit(main())
