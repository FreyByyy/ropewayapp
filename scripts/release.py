"""Publishes a Ropeway release on GitHub.

Put the built files into releases/<version>/ under their release names (see
docs/DEVELOPMENT.md), then run:

    python scripts/release.py            # check, sign, write latest.json
    python scripts/release.py --publish  # ... and create the GitHub release

Update packages are signed with the key in ~/.tauri/ropeway-updater.key
(override with ROPEWAY_UPDATER_KEY). The key never leaves this computer.
The release notes are the version's section of CHANGELOG.md.
"""

import datetime
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "FreyByyy/ropewayapp"
KEY = Path(os.environ.get("ROPEWAY_UPDATER_KEY", Path.home() / ".tauri" / "ropeway-updater.key"))

# latest.json platform → update package. macOS is one universal package.
UPDATE_PACKAGES = {
    "windows-x86_64": "windows-setup.exe",
    "darwin-aarch64": "macos-universal.app.tar.gz",
    "darwin-x86_64": "macos-universal.app.tar.gz",
    "linux-x86_64": "linux-x86_64.AppImage",
    "android-aarch64": "android-arm64.apk",
    "android-x86_64": "android-x86_64.apk",
}
# Downloads that are not update packages.
OTHER_FILES = [
    "windows-portable.exe",
    "macos-universal.dmg",
    "linux-amd64.deb",
    "linux-x86_64.rpm",
]


def version() -> str:
    return json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))["version"]


def notes(v: str) -> str:
    text = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    for part in text.split("\n## ")[1:]:
        head, _, body = part.partition("\n")
        if head.split()[0] == v:
            return body.strip()
    sys.exit(f"CHANGELOG.md has no section for {v}")


def sign(path: Path) -> str:
    sig = path.with_name(path.name + ".sig")
    if not sig.exists():
        env = dict(os.environ, TAURI_SIGNING_PRIVATE_KEY_PASSWORD="", CI="true")
        npx = shutil.which("npx") or "npx"
        subprocess.run([npx, "tauri", "signer", "sign", "-f", str(KEY), "-p", "", str(path)], cwd=ROOT, env=env, check=True)
    return sig.read_text(encoding="utf-8").strip()


def main() -> None:
    publish = "--publish" in sys.argv
    v = version()
    folder = ROOT / "releases" / v
    name = lambda suffix: f"Ropeway-{v}-{suffix}"  # noqa: E731

    missing = [name(s) for s in {*UPDATE_PACKAGES.values(), *OTHER_FILES} if not (folder / name(s)).exists()]
    if missing:
        sys.exit("missing in " + str(folder) + ":\n  " + "\n  ".join(sorted(missing)))
    if not KEY.exists():
        sys.exit(f"no update key at {KEY}")

    body = notes(v)
    base = f"https://github.com/{REPO}/releases/download/v{v}"
    platforms = {}
    for platform, suffix in UPDATE_PACKAGES.items():
        file = folder / name(suffix)
        platforms[platform] = {"signature": sign(file), "url": f"{base}/{file.name}"}
    manifest = {
        "version": v,
        "notes": body,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    (folder / "latest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    (folder / "notes.md").write_text(body + "\n", encoding="utf-8")
    print(f"latest.json written for {v}: {', '.join(platforms)}")

    if not publish:
        return
    assets = sorted({folder / name(s) for s in [*UPDATE_PACKAGES.values(), *OTHER_FILES]})
    assets += sorted(folder / (name(s) + ".sig") for s in set(UPDATE_PACKAGES.values()))
    assets.append(folder / "latest.json")
    gh = shutil.which("gh") or r"C:\Program Files\GitHub CLI\gh.exe"
    cmd = [gh, "release", "create", f"v{v}", "--repo", REPO, "--title", f"Ropeway {v}", "--notes-file", str(folder / "notes.md"), "--latest"]
    subprocess.run(cmd + [str(a) for a in assets], cwd=ROOT, check=True)
    print(f"published https://github.com/{REPO}/releases/tag/v{v}")


if __name__ == "__main__":
    main()
