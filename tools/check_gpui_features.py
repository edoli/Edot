"""Check target-specific feature unification, including test dependencies."""

import re
import subprocess
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[1]
manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
dependency = manifest["dependencies"]["gpui"]
git_url, revision = dependency["git"], dependency["rev"]
assert git_url == "https://github.com/edoli/zed.git", git_url
assert re.fullmatch(r"[0-9a-f]{40}", revision), revision
expected_source = f"git+{git_url}?rev={revision}#{revision}"
for section, name in [
    ("dependencies", "gpui"),
    ("dependencies", "gpui_platform"),
    ("dev-dependencies", "gpui"),
]:
    entry = manifest[section][name]
    assert entry["git"] == git_url and entry["rev"] == revision, (section, name)
    assert entry.get("default-features") is False, (section, name)
    assert "path" not in entry and "package" not in entry, (section, name)
lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
locked = {(entry["name"], entry["version"]): entry for entry in lock["package"]}
# Cargo metadata can report features unified across other platforms even with
# --filter-platform. Cargo tree resolves the actual Windows feature graph.
tree = subprocess.check_output(
    [
        "cargo",
        "tree",
        "--offline",
        "--locked",
        "--target",
        "x86_64-pc-windows-msvc",
        "--edges",
        "normal,build,dev",
        "--prefix",
        "none",
        "--format",
        "{p}\t{f}",
    ],
    cwd=root,
    encoding="utf-8",
)
packages = {}
for line in tree.splitlines():
    package, enabled = line.removesuffix(" (*)").split("\t", 1)
    name, version, *_ = package.split(" ", 2)
    active = {feature for feature in enabled.split(",") if feature}
    packages.setdefault((name, version), (package, set()))[1].update(active)


def package_entry(name):
    matches = [(key, value) for key, value in packages.items() if key[0] == name]
    assert len(matches) == 1, (name, matches)
    return matches[0]


def features(name):
    return package_entry(name)[1][1]


assert not any(entry["name"].startswith("gpui-pre") for entry in lock["package"])
for entry in lock["package"]:
    if entry.get("source", "").startswith(f"git+{git_url}?"):
        assert entry["source"] == expected_source, entry["name"]

for name in ["gpui", "gpui_platform", "gpui_windows", "gpui_macros"]:
    (_, version), (package, enabled) = package_entry(name)
    assert locked[name, version.removeprefix("v")]["source"] == expected_source, name
    assert f"({git_url}?rev={revision}#{revision[:8]})" in package, package

for name in ["gpui", "gpui_windows"]:
    enabled = features(name)
    assert not enabled & {
        "default",
        "extended-image-formats",
        "svg-text",
        "svg-raster-images",
    }, (name, enabled)

assert "windows-manifest" in features("gpui")
assert "deferred-fonts" in features("gpui_platform")
assert "deferred-fonts" in features("gpui_windows")
assert "test-support" in features("gpui")
assert features("image") == {"bmp", "gif", "jpeg", "png", "webp"}
assert not features("resvg") & {
    "default",
    "text",
    "system-fonts",
    "memmap-fonts",
    "raster-images",
}
assert not features("usvg") & {
    "default",
    "text",
    "system-fonts",
    "memmap-fonts",
}
print("GPUI Git pins, Windows manifest, image codecs and SVG feature limits verified.")
