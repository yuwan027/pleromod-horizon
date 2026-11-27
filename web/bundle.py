#!/usr/bin/env python3
"""Packs prepared resources for the browser build.

boot.tar.gz holds what the game reads before the title screen and for a
world (everything but other languages, sounds, the world generator's bundle
and caches the game rebuilds); manifest.txt lists the rest, which the game fetches
from res/ the first time it reads a file. The world generator's folders are
read together, hundreds of files when a world starts: each is also packed whole
(worldgen.tar.gz...), which the manifest names for its files, so the first read
fetches all of them in one request.

With REPLACED (content-replaced.txt, from content_root.py), the files it lists
are fetched when read too, each by itself: the game reads the copies in the
content root built ahead instead, and these only should it ever build that root
itself.

Usage: bundle.py RESOURCES OUT [REPLACED]
"""

import gzip
import hashlib
import os
import sys
import tarfile
from pathlib import Path

# Fetched when first read instead of downloaded up front.
LAZY_DIRS = ("worldgen", "biome-parameters")
LAZY_FILES = ("horizon-sprites.cache",)
# deprecated.json (the renamed keys) is read with every language at startup.
# The web build starts in zh_cn, and ja_jp is its neighbour on pleromod.com: both
# are read before the first frame, so they come with boot rather than lazily.
KEPT_LANGUAGES = ("en_us.json", "deprecated.json", "zh_cn.json", "ja_jp.json")
# Lazy folders fetched whole, as <folder>.tar.gz, when one of their files is first read.
PACKED_DIRS = ("worldgen", "biome-parameters")
# The browser plays the shared world of pleromod.com/play, which generates no terrain: the
# worldgen pack is what growth reads (trees' and mushrooms' features), not the structures'
# templates, which stay fetched one by one should anything read them.
UNPACKED = ("worldgen/structure/",)


def packed(rel: str) -> bool:
    return rel.split("/")[0] in PACKED_DIRS and not rel.startswith(UNPACKED)
# The browser build has only the Overworld: the structure templates of the Nether's and
# the End's structures (bastions, nether fossils, End cities) are left out altogether.
DROPPED_STRUCTURES = ("bastion", "end_city", "nether_fossils")


def dropped(rel: str) -> bool:
    parts = rel.split("/")
    # The browser build offers only zh_cn, en_us and ja_jp: other languages aren't served.
    if parts[:3] == ["assets", "minecraft", "lang"] and len(parts) == 4:
        return parts[3] not in KEPT_LANGUAGES
    return len(parts) > 3 and parts[3] in DROPPED_STRUCTURES and (
        parts[:3] == ["data", "minecraft", "structure"]
        or parts[:3] == ["worldgen", "structure", "minecraft"]
    )


def lazy(rel: str) -> bool:
    parts = rel.split("/")
    if parts[0] in LAZY_DIRS or rel in LAZY_FILES:
        return True
    if parts[:3] == ["assets", "minecraft", "sounds"]:
        return True
    if parts[:3] == ["assets", "minecraft", "lang"] and len(parts) == 4:
        return parts[3] not in KEPT_LANGUAGES
    return False


def main() -> None:
    resources, out = Path(sys.argv[1]), Path(sys.argv[2])
    if not (resources / "horizon-resources.json").is_file():
        sys.exit(f"{resources} holds no prepared resources: run horizon-prepare first")
    out.mkdir(parents=True, exist_ok=True)
    replaced = set(Path(sys.argv[3]).read_text().split()) if len(sys.argv) > 3 else set()
    boot, rest = [], []
    for dirpath, _, names in os.walk(resources):
        for name in names:
            path = Path(dirpath) / name
            rel = path.relative_to(resources).as_posix()
            if dropped(rel):
                continue
            (rest if lazy(rel) or rel in replaced else boot).append((rel, path))
    boot.sort()
    rest.sort()
    with tarfile.open(out / "boot.tar.gz", "w:gz", format=tarfile.GNU_FORMAT, compresslevel=6) as tar:
        for rel, path in boot:
            tar.add(path, arcname=rel, recursive=False)
    packs = {}
    for folder in PACKED_DIRS:
        files = [
            (rel, path)
            for rel, path in rest
            if rel.split("/")[0] == folder and packed(rel) and rel not in replaced
        ]
        if not files:
            continue
        name = f"{folder}.tar.gz"
        # mtime 0 and sorted names: the same files make the same archive, and the same hash.
        with open(out / name, "wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", compresslevel=9, mtime=0) as gz:
            with tarfile.open(fileobj=gz, mode="w", format=tarfile.GNU_FORMAT) as tar:
                for rel, path in files:
                    info = tar.gettarinfo(path, arcname=rel)
                    info.mtime, info.uid, info.gid, info.uname, info.gname = 0, 0, 0, "", ""
                    with open(path, "rb") as f:
                        tar.addfile(info, f)
        digest = hashlib.sha256((out / name).read_bytes()).hexdigest()[:16]
        # Relative to res/, where the manifest's files are fetched from; the
        # hash lets the service worker keep it until it changes.
        packs[folder] = f"../{name}?v={digest}"
    with open(out / "manifest.txt", "w") as manifest:
        for rel, path in rest:
            pack = packs.get(rel.split("/")[0]) if packed(rel) and rel not in replaced else None
            manifest.write(f"{rel}\t{path.stat().st_size}" + (f"\t{pack}" if pack else "") + "\n")
    res = out / "res"
    if res.is_symlink() or res.exists():
        res.unlink()
    res.symlink_to(resources.resolve(), target_is_directory=True)
    size = (out / "boot.tar.gz").stat().st_size
    print(f"boot.tar.gz: {len(boot)} files, {size / 1e6:.1f} MB; {len(rest)} fetched when read")


if __name__ == "__main__":
    main()
