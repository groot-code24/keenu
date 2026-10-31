#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Installs, upgrades and removes real packages on this Windows computer, under this user.

It refuses to run if Keenu is already installed here. Data goes to a temporary folder; the install folder is
the real one, and is removed again at the end whatever happens.

    python scripts/windows/installer_test.py --msi dist/new.msi --old-msi dist/old.msi --setup dist/setup.exe \
        --zip dist/portable.zip --release-tool keenu-release.exe --version 2.0.0-alpha.1
"""
import argparse
import http.server
import os
import pathlib
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import winreg

SYSTEM = pathlib.Path(os.environ["SystemRoot"]) / "System32"
MSIEXEC = SYSTEM / "msiexec.exe"
POWERSHELL = SYSTEM / "WindowsPowerShell" / "v1.0" / "powershell.exe"
INSTALL_DIR = pathlib.Path(os.environ["LOCALAPPDATA"]) / "Programs" / "Keenu"
EXE = INSTALL_DIR / "keenu.exe"
SHORTCUT = pathlib.Path(os.environ["APPDATA"]) / "Microsoft/Windows/Start Menu/Programs/Keenu.lnk"
APP_PATH = r"Software\Microsoft\Windows\CurrentVersion\App Paths\keenu.exe"
HERE = pathlib.Path(__file__).resolve().parent

passes = 0
failures = 0


def check(name, condition, why=""):
    global passes, failures
    if condition:
        passes += 1
        print(f"PASS  {name}", flush=True)
    else:
        failures += 1
        print(f"FAIL  {name} : {why}", flush=True)


def run(args, **kwargs):
    return subprocess.run(args, capture_output=True, text=True, errors="replace", **kwargs)


def keenu(*args):
    done = run([str(EXE), *args])
    return done.returncode, done.stdout.strip()


def msi(flag, file):
    return run([str(MSIEXEC), flag, str(pathlib.Path(file).resolve()), "/qn", "/norestart"]).returncode


def apps_entries():
    """Installed Keenus: a setup program lists itself under Uninstall, a per-user MSI under Products."""
    count = 0
    for path, value in (
        (r"Software\Microsoft\Windows\CurrentVersion\Uninstall", "DisplayName"),
        (r"Software\Microsoft\Installer\Products", "ProductName"),
    ):
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
                for index in range(winreg.QueryInfoKey(key)[0]):
                    with winreg.OpenKey(key, winreg.EnumKey(key, index)) as entry:
                        try:
                            if winreg.QueryValueEx(entry, value)[0] == "Keenu":
                                count += 1
                        except OSError:
                            pass
        except OSError:
            pass
    return count


def has_app_path():
    try:
        winreg.CloseKey(winreg.OpenKey(winreg.HKEY_CURRENT_USER, APP_PATH))
        return True
    except OSError:
        return False


def user_path():
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        try:
            return winreg.QueryValueEx(key, "Path")[0]
        except OSError:
            return ""


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass


def serve(folder):
    handler = lambda *args, **kwargs: Quiet(*args, directory=str(folder), **kwargs)
    server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def installer(env, *args):
    script = HERE.parent / "install" / "install.ps1"
    done = run([str(POWERSHELL), "-NoProfile", "-File", str(script), *args], env=env)
    return done.returncode, done.stdout + done.stderr


def test_msi(args):
    check("the older MSI installs without administrator rights", msi("/i", args.old_msi) in (0, 3010))
    check("it puts keenu.exe in place", EXE.exists())
    check("it adds a Start Menu shortcut", SHORTCUT.exists())
    check("it can be found by name", has_app_path())
    check("one entry in Apps", apps_entries() == 1, apps_entries())
    code, out = keenu("--todo-add", "Keep me across the upgrade")
    check("the installed program runs", code == 0 and "Created task" in out, out)

    check("the newer MSI upgrades it", msi("/i", args.msi) in (0, 3010))
    check("still one entry in Apps", apps_entries() == 1, apps_entries())
    check("the upgrade keeps the data", "Keep me across the upgrade" in keenu("--todo-list")[1])
    check("the upgrade is the new version", keenu("--version")[1] == args.version, keenu("--version")[1])
    down = msi("/i", args.old_msi)
    check("an older MSI is refused over a newer one", down not in (0, 3010), f"exit {down}")
    check("and leaves the new version in place", keenu("--version")[1] == args.version)

    check("the MSI uninstalls", msi("/x", args.msi) in (0, 3010))
    check("the program is gone", not EXE.exists())
    check("the shortcut is gone", not SHORTCUT.exists())
    check("no entry left in Apps", apps_entries() == 0, apps_entries())
    check("its name is gone", not has_app_path())
    data = pathlib.Path(os.environ["KEENU_DATA_DIR"])
    check("the data is still there", (data / "todo.json").exists())


def test_setup(args):
    code = run([str(pathlib.Path(args.setup).resolve()), "/S"]).returncode
    check("the setup program installs silently", code == 0 and EXE.exists(), f"exit {code}")
    check("and the program runs", keenu("--version")[1] == args.version)
    check("and keeps the data", "Keep me across" in keenu("--todo-list")[1])
    run([str(INSTALL_DIR / "uninstall.exe"), "/S"])
    # The uninstaller hands over to a copy of itself and returns before it has finished.
    for _ in range(50):
        if not EXE.exists() and apps_entries() == 0:
            break
        threading.Event().wait(0.2)
    check("the setup program uninstalls", not EXE.exists())
    check("its Apps entry is gone", apps_entries() == 0, apps_entries())


def test_one_command(args, work):
    folder = work / "release"
    folder.mkdir()
    shutil.copy(args.msi, folder)
    shutil.copy(args.zip, folder)
    server = serve(folder)
    port = server.server_address[1]
    try:
        made = run([args.release_tool, "manifest", str(folder), "--version", args.version,
                    "--base-url", f"http://127.0.0.1:{port}", "--out", str(folder / "keenu-release.json")])
        check("the release tool lists what is there", made.returncode == 0, made.stderr)

        env = dict(os.environ, KEENU_YES="1", KEENU_NO_START="1", KEENU_MANIFEST_URL=f"http://127.0.0.1:{port}/keenu-release.json")
        code, out = installer(env)
        check("the one-command installer installs the release", code == 0 and EXE.exists(), out)
        check("it verified the checksum", "Checksum verified" in out, out)
        check("it says the build is not signed", "not code signed" in out, out)
        check("the installed version is the release", keenu("--version")[1] == args.version)
        code, out = installer(env)
        check("running it again keeps settings and says so", code == 0 and "will be kept" in out, out)
        check("and the data", "Keep me across" in keenu("--todo-list")[1])
        code, out = installer(env, "-Uninstall")
        check("it removes Keenu again", code == 0 and not EXE.exists(), out)

        # A file changed on the server, as a compromised mirror would.
        served = folder / pathlib.Path(args.msi).name
        data = bytearray(served.read_bytes())
        data[-100] ^= 0xFF
        served.write_bytes(data)
        code, out = installer(env)
        check("a changed download is refused and not installed", code != 0 and "does not match" in out and not EXE.exists(), out)

        env["KEENU_MANIFEST_URL"] = "http://example.org/keenu-release.json"
        code, out = installer(env)
        check("an insecure release address is refused", code != 0 and "not a secure address" in out, out)

        env["KEENU_MANIFEST_URL"] = f"http://127.0.0.1:{port}/nothing-here.json"
        code, out = installer(env)
        check("a release list that is not there is a clear error", code != 0 and "could not fetch" in out, out)
        script = HERE.parent / "install" / "install.ps1"
        done = run([str(POWERSHELL), "-NoProfile", "-Command", f"Get-Content -Raw '{script}' | Invoke-Expression; Write-Host window-still-open"], env=env)
        check("a failure through iex leaves the window open, with the reason", "could not fetch" in done.stdout and "window-still-open" in done.stdout, done.stdout + done.stderr)
    finally:
        server.shutdown()


def main():
    parser = argparse.ArgumentParser()
    for name in ("msi", "old-msi", "setup", "zip", "release-tool", "version"):
        parser.add_argument(f"--{name}", required=True)
    args = parser.parse_args()
    if EXE.exists():
        sys.exit(f"Keenu is already installed at {INSTALL_DIR}; this test will not touch a real install.")
    path_before = user_path()
    work = pathlib.Path(tempfile.mkdtemp(prefix="keenu-installer-test-"))
    os.environ["KEENU_DATA_DIR"] = str(work / "data")
    # Uninstalling takes Keenu's hooks out of the agents' settings: only ever these, not the user's real ones.
    os.environ["CLAUDE_CONFIG_DIR"] = str(work / "claude")
    os.environ["CODEX_HOME"] = str(work / "codex")
    try:
        test_msi(args)
        test_setup(args)
        test_one_command(args, work)
        check("PATH was left as it was", user_path() == path_before)
    finally:
        # The computer as it was, whatever happened above.
        if (INSTALL_DIR / "uninstall.exe").exists():
            run([str(INSTALL_DIR / "uninstall.exe"), "/S"])
        if EXE.exists():
            msi("/x", args.msi)
        shutil.rmtree(work, ignore_errors=True)
    print(f"\n{passes} passed, {failures} failed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
