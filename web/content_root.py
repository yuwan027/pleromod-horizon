#!/usr/bin/env python3
"""Packs the content root the native game built ahead for the page (build.sh).

The page's game lays its mods' content over the prepared resources at every
start: nothing it builds is kept between visits, and rewriting the per-state
tables for the mods' blocks takes seconds before anything can load. The native
game builds the same root here (HORIZON_WEB_CONTENT), and this packs what the
build wrote (the rewritten tables, merged tags, languages and atlases) as
content.tar.gz, with a list of what the root only links to: the prepared
resources' files and the mods' own, which the page has already. The game makes
those links when it finds the root with its own stamp, and builds the root
itself when it doesn't (other mods, or another build of the game).

The prepared resources' files that the root replaces with its own are listed in
content-replaced.txt beside the archive: the game reads the root's, so bundle.py
leaves those out of what the page downloads up front.

Usage: content_root.py ROOT RESOURCES OUT ID=MOD_FOLDER...
"""

import gzip
import hashlib
import io
import os
import sys
import tarfile
from pathlib import Path

# As crates/horizon-client/src/mod_content.rs names them.
STAMP = "horizon-mod-content.stamp"
PREBUILT = "horizon-mod-content.prebuilt"
# Where the page has the prepared resources (main.js's ROOT) and the mods' content.
PAGE_RESOURCES = "/.local/horizon-client/26.3"
PAGE_MODS = "/native-content"
# Roots in the page's game folder, as `resource_root` names them.
PAGE_ROOTS = ".horizon-cache/mod-content"


def digest(path: Path) -> bytes:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.digest()


def main() -> None:
    root, resources, out = Path(sys.argv[1]), Path(sys.argv[2]).resolve(), Path(sys.argv[3])
    mods = {arg.split("=", 1)[0]: Path(arg.split("=", 1)[1]).resolve() for arg in sys.argv[4:]}
    stamp = (root / STAMP).read_text()
    # The mods' files by content: the root holds copies of them under the same or another name.
    theirs = {}
    for mod, folder in mods.items():
        for dirpath, _, names in os.walk(folder):
            for name in names:
                path = Path(dirpath, name)
                theirs.setdefault(
                    (path.stat().st_size, digest(path)),
                    f"{PAGE_MODS}/{mod}/{path.relative_to(folder).as_posix()}",
                )

    def on_page(target: Path) -> str:
        target = target.resolve()
        if target.is_relative_to(resources):
            return f"{PAGE_RESOURCES}/{target.relative_to(resources).as_posix()}".rstrip("/")
        for mod, folder in mods.items():
            if target.is_relative_to(folder):
                return f"{PAGE_MODS}/{mod}/{target.relative_to(folder).as_posix()}".rstrip("/")
        sys.exit(f"{target}: linked from the content root, but in neither the resources nor a mod")

    stamp_folder = f"/{PAGE_ROOTS}/{stamp[:16]}"
    links, files, written = [], [], {}
    for dirpath, dirnames, names in os.walk(root):
        for name in sorted(dirnames + names):
            path = Path(dirpath, name)
            rel = path.relative_to(root).as_posix()
            if path.is_symlink():
                links.append((rel, on_page(path)))
            elif path.is_file() and rel != STAMP:
                content = (path.stat().st_size, digest(path))
                # A mod's own file, or one the build wrote twice (the world generator's
                # copies of the collision shapes and the block tags): packed once.
                same = theirs.get(content) or written.get(content)
                if same:
                    links.append((rel, same))
                else:
                    files.append((rel, path))
                    written[content] = f"{stamp_folder}/{rel}"
        # A linked folder is one link, not walked.
        dirnames[:] = sorted(d for d in dirnames if not Path(dirpath, d).is_symlink())
    links.sort()
    files.sort()
    listing = "".join(f"{line}\n" for line in [stamp] + [f"{rel}\t{to}" for rel, to in links]).encode()
    folder = stamp_folder[1:]
    # mtime 0 and sorted names: the same root makes the same archive, and the same hash.
    with open(out, "wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", compresslevel=9, mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.GNU_FORMAT) as tar:
            info = tarfile.TarInfo(f"{folder}/{PREBUILT}")
            info.size = len(listing)
            tar.addfile(info, io.BytesIO(listing))
            for rel, path in files:
                info = tarfile.TarInfo(f"{folder}/{rel}")
                info.size = path.stat().st_size
                with open(path, "rb") as f:
                    tar.addfile(info, f)
    # Everything of the root that is not the resources' own file: what the build wrote,
    # once or twice, and the mods' files over the resources'.
    own = [rel for rel, _ in files] + [rel for rel, to in links if not to.startswith(PAGE_RESOURCES + "/")]
    replaced = "".join(f"{rel}\n" for rel in sorted(own) if (resources / rel).is_file())
    listed = out.with_name("content-replaced.txt")
    # Only when it changed: build.sh packs the resources again when this file is newer.
    if not listed.is_file() or listed.read_text() != replaced:
        listed.write_text(replaced)
    size = sum(path.stat().st_size for _, path in files)
    print(
        f"content.tar.gz: {len(files)} files written by the build ({size / 1e6:.1f} MB), "
        f"{len(links)} links, {out.stat().st_size / 1e6:.2f} MB"
    )


if __name__ == "__main__":
    main()
