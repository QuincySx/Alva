# INPUT: Explicit WASI SDK clang, checksum-pinned QuickJS source archive and local bridge/bootstrap
# OUTPUT: Reproducible bounded WASIp1 reactor plus binary/source/build provenance
# POS: [platform / WASM host] Maintainer-only build, never an application startup download or native runtime dependency
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import tarfile
import tempfile

ENGINE_VERSION = "0.17.0"
SOURCE_SHA256 = "559bc4c420475e55c7ab4510adbc562f55d7524d75e8e89d79ce4bb02f5687d9"
SOURCE_URL = f"https://codeload.github.com/quickjs-ng/quickjs/tar.gz/refs/tags/v{ENGINE_VERSION}"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--clang", required=True)
    parser.add_argument("--source-tar", required=True)
    args = parser.parse_args()
    here = pathlib.Path(__file__).resolve().parent
    source = pathlib.Path(args.source_tar).read_bytes()
    if hashlib.sha256(source).hexdigest() != SOURCE_SHA256:
        raise SystemExit("pinned QuickJS source checksum mismatch")
    clang = pathlib.Path(args.clang).resolve(strict=True)
    version = subprocess.check_output([str(clang), "--version"], text=True).splitlines()[0]
    with tempfile.TemporaryDirectory(prefix="alva-qjs-rebuild-") as directory:
        build = pathlib.Path(directory)
        with tarfile.open(args.source_tar) as archive:
            for member in archive.getmembers():
                parts = pathlib.PurePosixPath(member.name).parts
                if not parts or parts[0] != f"quickjs-{ENGINE_VERSION}" or ".." in parts:
                    raise SystemExit("unexpected source archive path")
                if member.isdir():
                    continue
                if not member.isfile():
                    raise SystemExit("source archive contains a non-regular entry")
                destination = build.joinpath(*parts[1:])
                destination.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as src, destination.open("wb") as dst:
                    shutil.copyfileobj(src, dst)
        bootstrap = (here / "bootstrap.js").read_text()
        (build / "bootstrap.h").write_text("static const char bootstrap[] =\n" + "\n".join(json.dumps(line + "\n") for line in bootstrap.splitlines()) + ";\n")
        shutil.copyfile(here / "bridge.c", build / "bridge.c")
        flags = ["--target=wasm32-wasip1", "-mexec-model=reactor", "-O2", "-DNDEBUG", "-D_GNU_SOURCE", "-D_WASI_EMULATED_SIGNAL", "-mno-tail-call", "-ffile-prefix-map=" + str(build) + "=/alva-quickjs-source", "-I", str(build), "-Wl,-z,stack-size=2097152", "-Wl,--initial-memory=4194304", "-Wl,--max-memory=67108864", "-Wl,--strip-all"]
        output = build / "quickjs.wasm"
        units = [build / name for name in ["bridge.c", "quickjs.c", "dtoa.c", "libregexp.c", "libunicode.c"]]
        subprocess.run([str(clang), *flags, *map(str, units), "-lm", "-lwasi-emulated-signal", "-o", str(output)], check=True)
        binary = output.read_bytes()
        if len(binary) > 1 << 20:
            raise SystemExit("QuickJS core exceeds the 1MiB asset budget; audit required")
        (here / "quickjs.wasm").write_bytes(binary)
        shutil.copyfile(build / "LICENSE", here / "LICENSE.quickjs")
        provenance = {"version": 1, "engine": f"QuickJS-NG v{ENGINE_VERSION} core only", "source_url": SOURCE_URL, "source_sha256": SOURCE_SHA256, "compiler": version, "wasi_sdk": "34.0", "wasi_sdk_arm64_macos_sha256": "9c59398106b417f8f14913380fdf0097a8cc0ff4af9eb3ce0065a859e88d49e9", "bridge_sha256": hashlib.sha256((here / "bridge.c").read_bytes()).hexdigest(), "bootstrap_sha256": hashlib.sha256((here / "bootstrap.js").read_bytes()).hexdigest(), "binary_sha256": hashlib.sha256(binary).hexdigest(), "binary_bytes": len(binary), "patch": "None; upstream WASI stack guard; no quickjs-libc, module loader or std/os module linked", "limits": {"quickjs_heap_bytes": 32 << 20, "quickjs_soft_c_stack_bytes": 512 << 10, "wasm_linear_memory_bytes": 64 << 20}, "command": f"python3 build.py --clang /explicit/wasi-sdk-34/bin/clang --source-tar /explicit/quickjs-v{ENGINE_VERSION}.tar.gz", "flags": [flag.replace(str(build), "/alva-quickjs-source") for flag in flags]}
        (here / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        print(json.dumps({"sha256": provenance["binary_sha256"], "bytes": len(binary), "compiler": version}))


if __name__ == "__main__":
    main()
