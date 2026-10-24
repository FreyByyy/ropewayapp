"""Downloads the app's icons from Icons8 (https://icons8.com), one set per theme style.

Run from the project root:  python scripts/fetch-icons.py [name ...]
With names, only those icons are fetched and merged into the manifest.
Icons go to src/assets/icons/<set>/<name>.png; src/assets/icons/manifest.json
records which Icons8 icon each file is. Pin an icon by putting its id in
OVERRIDES when the first search result is not the right one.
"""

import json
import os
import sys
import time
import urllib.parse
import urllib.request

SETS = {
    "fluent": "fluent-systems-regular",
    "material": "m_rounded",
    "puffy": "puffy",
    "glass": "sf-regular",
    "retro": "retro",
}

# Sets drawn in their own colors instead of as a mask of the text color.
COLOR_SETS = {"retro"}

# name -> search term
ICONS = {
    "general": "group",
    "chat": "chat",
    "folder": "folder",
    "settings": "settings",
    "plus": "plus math",
    "radar": "radar",
    "send": "sent",
    "check": "checkmark",
    "clock": "clock",
    "alert": "error",
    "file": "file",
    "cast": "present to all",
    "pointer": "cursor",
    "shield": "shield",
    "shield-check": "security checked",
    "close": "multiply",
    "download": "download",
    "upload": "upload",
    "pencil": "edit",
    "trash": "trash",
    "external": "external link",
    "chevron-right": "forward",
    "refresh": "refresh",
    "monitor": "monitor",
    "menu": "menu",
    "palette": "paint palette",
    "laptop": "laptop",
    "sun": "sun",
    "moon": "moon",
    "speaker": "speaker",
    "mute": "mute",
    "smile": "smiling",
    "star": "star",
    "qr": "qr code",
    "scan": "camera",
    "github": "github",
    "devices": "multiple devices",
}

# (set, name) -> search term for one style.
TERM_OVERRIDES: dict[tuple[str, str], str] = {}

# (set, name) -> icon id, for when search picks the wrong icon.
OVERRIDES: dict[tuple[str, str], str] = {
    # Plain checkmark (delivered), not a check in a circle.
    ("fluent", "check"): "VfY8rwjNJSAQ",
    ("material", "check"): "83145",
    ("puffy", "check"): "0AUe0fpTPlTW",
    # "Looking for devices": network waves rather than a satellite dish.
    ("material", "radar"): "85509",
    ("puffy", "radar"): "nu1kAaBAozFU",
    # Trusted device: shield with a check.
    ("fluent", "shield-check"): "bfwHrORtPSpB",
    ("material", "shield-check"): "85804",
    ("puffy", "shield-check"): "JWkambf71CS5",
    ("puffy", "plus"): "DcsOezeH3xZF",
    ("puffy", "speaker"): "qHNrgJy2n0Cl",
    ("puffy", "cast"): "IXsYFW96PRXP",
    # Retro has no "sent" arrow.
    ("retro", "send"): "ZYyrrpLqriXB",
    # Screen sharing: a laptop with a cursor.
    ("glass", "cast"): "dm4a1Qqf1OOK",
    ("retro", "cast"): "aH5JoO85Plw6",
    ("glass", "shield-check"): "d4Ew6ImCLbxm",
    # SF Symbols has no crossed-out speaker; this one is from iOS Outlined.
    ("glass", "mute"): "644",
}

ROOT = os.path.join(os.path.dirname(__file__), "..", "src", "assets", "icons")


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "ropeway-icon-fetch"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


def search(term: str, platform: str, color: bool) -> list[dict]:
    q = urllib.parse.urlencode({"term": term, "amount": 10, "platform": platform})
    data = json.loads(get(f"https://search.icons8.com/api/iconsets/v5/search?{q}"))
    return [i for i in data.get("icons", []) if bool(i.get("isColor")) == color and not i.get("isExplicit")]


def main() -> int:
    only = set(sys.argv[1:])
    manifest_path = os.path.join(ROOT, "manifest.json")
    manifest: dict[str, dict[str, dict]] = {}
    if only and os.path.exists(manifest_path):
        with open(manifest_path, encoding="utf-8") as f:
            manifest = json.load(f)["icons"]
    failed = []
    for set_name, platform in SETS.items():
        os.makedirs(os.path.join(ROOT, set_name), exist_ok=True)
        manifest.setdefault(set_name, {})
        for name, term in ICONS.items():
            if only and name not in only:
                continue
            icon_id = OVERRIDES.get((set_name, name))
            common = None
            if not icon_id:
                results = search(TERM_OVERRIDES.get((set_name, name), term), platform, set_name in COLOR_SETS)
                if not results:
                    failed.append(f"{set_name}/{name} ({term})")
                    continue
                icon_id, common = results[0]["id"], results[0]["commonName"]
            png = get(f"https://img.icons8.com/?id={icon_id}&format=png&size=96")
            with open(os.path.join(ROOT, set_name, f"{name}.png"), "wb") as f:
                f.write(png)
            manifest[set_name][name] = {"id": icon_id, "name": common, "platform": platform}
            time.sleep(0.05)
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump({"source": "https://icons8.com", "icons": manifest}, f, indent=2)
    if failed:
        print("not found:", ", ".join(failed))
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
