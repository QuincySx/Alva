# INPUT: Explicit --run macOS QA request, pinned Go toolchain and independent native diagnostic API
# OUTPUT: Temporary own-app bundle/data/logs and separately reported callback/render evidence without credentials
# POS: Manual native acceptance runs demo only and never substitutes retained pixels for a current rendered frame.

import argparse
import base64
import json
import os
import pathlib
import plistlib
import re
import secrets
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description="Explicit local demo QA; opens and terminates only its own temporary native app.")
    parser.add_argument("--run", action="store_true", required=True)
    parser.add_argument("--app", choices=("direct", "hub"), default="hub")
    parser.add_argument("--launchservices", action="store_true", help="Ad-hoc sign only the own temporary bundle and launch it through macOS open.")
    parser.add_argument("--hold-seconds", type=int, default=0, help="Keep only this QA window alive for optional own-app CUA inspection, at most60 seconds.")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("This bundle/launch probe is macOS-only; Windows has a separate native CI job.")
    if not 0 <= args.hold_seconds <= 60:
        parser.error("hold-seconds must be0..60")
    module = pathlib.Path(__file__).resolve().parent.parent
    root = pathlib.Path(tempfile.mkdtemp(prefix="alva-native-window-qa-"))
    print("Own QA artifacts: " + str(root), flush=True)
    bundle = root / "AlvaWindowQA.app"
    executable = bundle / "Contents" / "MacOS" / "alva-window-qa"
    executable.parent.mkdir(parents=True)
    with (bundle / "Contents" / "Info.plist").open("wb") as output:
        plistlib.dump({"CFBundleExecutable": executable.name, "CFBundleIdentifier": "work.alva.window.qa." + str(time.time_ns()), "CFBundleName": "AlvaWindowQA", "CFBundlePackageType": "APPL", "NSHighResolutionCapable": True}, output)
    command = "alva-hub-desktop" if args.app == "hub" else "alva-desktop"
    environment = dict(os.environ, CGO_ENABLED="0", GOPROXY="off", GOTOOLCHAIN="local")
    subprocess.run(["go", "build", "-mod=readonly", "-tags", "desktop", "-o", str(executable), "./apps/cmd/" + command], cwd=module, env=environment, check=True)
    credential = base64.urlsafe_b64encode(secrets.token_bytes(32)).rstrip(b"=").decode()
    invocation = [str(executable), "--demo", "--debug-listen", "127.0.0.1:0", "--debug-controls", "--debug-execution", "--startup-secrets-stdin"]
    invocation += ["--data-dir", str(root / "data")]
    endpoint = None
    app_pid = None
    close_request = None
    with (root / "native.log").open("wb") as output:
        app_environment = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8", "TMPDIR": tempfile.gettempdir()}
        startup = root / "startup-input.json"
        if args.launchservices:
            startup.write_text(json.dumps({"debug_token": credential}))
            startup.chmod(0o600)
            subprocess.run(["/usr/bin/codesign", "--force", "--deep", "--sign", "-", str(bundle)], check=True)
            child = subprocess.Popen(["/usr/bin/open", "-n", "-W", "--stdin", str(startup), "--stdout", str(root / "stdout.log"), "--stderr", str(root / "native.log"), str(bundle), "--args", *invocation[1:]], stdin=subprocess.DEVNULL, stdout=output, stderr=output, env=app_environment)
        else:
            child = subprocess.Popen(invocation, stdin=subprocess.PIPE, stdout=output, stderr=output, env=app_environment)
        try:
            if not args.launchservices:
                child.stdin.write(json.dumps({"debug_token": credential}).encode())
                child.stdin.close()
            deadline = time.monotonic() + 40

            def wait_for(probe):
                while time.monotonic() < deadline:
                    if child.poll() is not None:
                        raise RuntimeError("Own QA application exited; inspect native.log.")
                    value = probe()
                    if value:
                        return value
                    time.sleep(0.1)
                raise RuntimeError("Native QA deadline exceeded; no success inferred from ready/visible alone.")

            def endpoint_probe():
                found = re.search(r"ALVA UI diagnostics: (http://127\.0\.0\.1:\d+)", (root / "native.log").read_text(errors="replace"))
                return found.group(1) if found else None

            endpoint = wait_for(endpoint_probe)
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

            def request(path, data=None):
                body = None if data is None else json.dumps(data).encode()
                req = urllib.request.Request(endpoint + path, data=body, headers={"Authorization": "Bearer " + credential, "Content-Type": "application/json"})
                with opener.open(req, timeout=7) as response:
                    return response.read(4 * 1024 * 1024 + 1)

            app_pid = json.loads(request("/v1/debug/health"))["process_id"]
            if startup.exists():
                startup.unlink()

            views = wait_for(lambda: json.loads(request("/v1/debug/views"))["views"])
            view_id = views[0]["id"]

            def snapshot():
                return json.loads(request("/v1/debug/state?view=" + view_id))

            operation = 0

            def action(method, values):
                nonlocal operation
                operation += 1
                before = snapshot()
                invocation = {"operation_id": "qa-" + str(operation), "view_id": view_id, "revision": before["revision"], "method": method, "args": values}
                outcome = json.loads(request("/v1/debug/actions", invocation))
                if outcome["status"] != "applied":
                    raise RuntimeError("Actual UI action did not apply: " + method + "/" + outcome["status"])
                return invocation

            close_request = lambda: action("close_window", {})

            action("create_session", {"title": "Native local demo acceptance"})
            wait_for(lambda: (lambda state: state["session_id"] and not state["busy"] and not state["composer_busy"])(snapshot()["state"]))
            action("set_draft", {"text": "Native window local demo acceptance"})
            action("send", {})
            wait_for(lambda: (lambda state: state["status"] == "completed" and not state["busy"] and not state.get("composer_busy", False) and not state["active_run_id"])(snapshot()["state"]))
            if args.app == "hub":
                # Explicit history load is valid even if an occluded surface
                # has not built the next chat frame and advanced follow-end.
                wait_for(lambda: snapshot()["state"]["can_latest"])
                action("latest", {})
            final = wait_for(lambda: (lambda state: state if state["message_rows"] >= 2 else None)(snapshot()["state"]))
            result = {"app": args.app, "actual_window_thread_actions": True, "local_demo_completed": True, "message_rows": final["message_rows"], "compositor_or_ax_acceptance": False}
            if args.app == "hub":
                action("open_pane", {"pane": "inspector"})
            else:
                action("inspector", {})
            wait_for(lambda: (lambda state: not state.get("pane_busy", False) and not state.get("inspector_busy", False))(snapshot()["state"]))
            export_action = action("export_inspector", {})
            wait_for(lambda: (lambda state: not state["export_busy"] and state["has_export"])(snapshot()["state"]))
            replay = json.loads(request("/v1/debug/actions", export_action))
            exports = list((root / "data" / "exports").glob("*.jsonl"))
            if replay["status"] != "applied" or len(exports) != 1:
                raise RuntimeError("Native export action was replayed or failed to write its actual file.")
            bundle_data = exports[0].read_bytes()
            if credential.encode() in bundle_data or json.loads(bundle_data.splitlines()[-1])["type"] != "complete":
                raise RuntimeError("Invalid or private native JSONL bundle.")
            result["native_jsonl_export"] = {"path": str(exports[0]), "bytes": len(bundle_data), "same_operation_did_not_replay": True}
            try:
                pixels = request("/v1/debug/capture", {"view_id": view_id})
                if len(pixels) > 4 * 1024 * 1024 or not pixels.startswith(b"\x89PNG\r\n\x1a\n"):
                    raise RuntimeError("Invalid or over-budget own-window capture.")
                (root / "window-content.png").write_bytes(pixels)
                result["capture"] = "actual native window Content; current Build verified, compositor/AX unverified"
                result["png_bytes"] = len(pixels)
            except urllib.error.HTTPError as rejection:
                body = rejection.read(1024)
                if rejection.code != 400 or b"has not rendered its current view state" not in body:
                    raise RuntimeError("Unexpected diagnostic capture rejection.") from None
                result["capture"] = "frame_pending; previous scene correctly refused, visual acceptance incomplete"
            (root / "result.json").write_text(json.dumps(result, indent=2))
            print(json.dumps(result))
            print("Own QA artifacts: " + str(root))
            sys.stdout.flush()
            if args.hold_seconds:
                time.sleep(args.hold_seconds)
        finally:
            if close_request is not None:
                try:
                    close_request()
                except Exception:
                    pass  # Normal final-window Close can close HTTP before its acknowledgement.
            try:
                child.wait(timeout=8)
            except subprocess.TimeoutExpired:
                if args.launchservices:
                    candidates = [app_pid] if isinstance(app_pid, int) and app_pid > 0 else []
                    if not candidates:
                        found = subprocess.run(["/usr/bin/pgrep", "-f", "^" + re.escape(str(executable)) + "([[:space:]]|$)"], capture_output=True, text=True)
                        candidates = [int(value) for value in found.stdout.split() if value.isdigit()]
                    for pid in candidates:
                        identity = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "command="], capture_output=True, text=True).stdout.strip()
                        if identity == str(executable) or identity.startswith(str(executable) + " "):
                            os.kill(pid, signal.SIGTERM)
                elif child.poll() is None:
                    child.send_signal(signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=5)
            if startup.exists():
                startup.unlink()


if __name__ == "__main__":
    main()
