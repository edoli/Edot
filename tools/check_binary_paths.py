"""Reject absolute source/build paths and local build roots in a release EXE."""

import argparse
import json
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "exe", type=Path, nargs="?", default=ROOT / "target/release/edot.exe"
    )
    args = parser.parse_args()
    data = args.exe.read_bytes()
    config = tomllib.loads(
        (ROOT / ".cargo/path-remap.toml").read_text(encoding="utf-8")
    )
    flags = config["target"]["x86_64-pc-windows-msvc"]["rustflags"]
    prefixes = {
        flag.removeprefix("--remap-path-prefix=").rsplit("=", 1)[0]
        for flag in flags
        if flag.startswith("--remap-path-prefix=")
    }
    prefixes.add(str(Path.home()))
    problems = set()
    for encoding in ("utf-8", "utf-16le", "utf-16be"):
        text = data.decode(encoding, errors="ignore")
        folded = text.casefold()
        for prefix in prefixes:
            if prefix.casefold() in folded:
                problems.add(f"{encoding}: local build root {prefix}")
        for match in re.finditer(
            # Rust can concatenate URL constants with source strings. Do not
            # mistake the final "p://" in "http://" for a Windows drive.
            r"[A-Za-z]:(?:\\|/(?!/))[\x20-\x7e]{1,300}?\.(?:rs|pdb|cpp|c|h|hlsl)\b",
            text,
        ):
            problems.add(f"{encoding}: absolute source path {match.group()}")
    if problems:
        raise SystemExit("Unexpected build paths:\n" + "\n".join(sorted(problems)))
    print(
        json.dumps(
            {"exe": str(args.exe), "bytes": len(data), "absolute_build_paths": 0}
        )
    )


if __name__ == "__main__":
    main()
