#!/usr/bin/env bash
# Builds Horizon for the browser into web/dist: the game as WebAssembly, the
# page, and the prepared resources packed for download.
#
#   web/build.sh            the release build: LTO, then wasm-opt (binaryen) when installed
#   PROFILE=fast web/build.sh  optimized, no LTO or wasm-opt: quick to rebuild while working
#   WASM_OPT=0 web/build.sh skip wasm-opt
#   DIST=<folder> web/build.sh  build into another folder
#   BUNDLE=0 web/build.sh   only the game and the page, not the resources
#
# Threads are Web Workers on shared memory, which std only supports when it is
# rebuilt with atomics: that build needs the pinned nightly below with its
# rust-src (rustup component add rust-src --toolchain <it>).
# Needs: rustup target add wasm32-unknown-unknown, and the wasm-bindgen CLI of
# the version in Cargo.lock (cargo install wasm-bindgen-cli --version <it>); for
# the release build, wasm-opt (binaryen's package, e.g. pacman -S binaryen).
set -euo pipefail
cd "$(dirname "$0")/.."

profile="${PROFILE:-release}"
resources="${HORIZON_RESOURCES:-.local/horizon-client/26.3}"
dist="${DIST:-web/dist}"

nightly=nightly-2026-10-14
# Replaces .cargo/config.toml's flags for the target, so it repeats them.
# The linker flags give the module a shared memory the page hands each
# worker, and the thread-local storage symbols wasm-bindgen sets up per thread.
flags=(
    '--cfg getrandom_backend="wasm_js"'
    # simd128: 128-bit vectors (glam's maths, autovectorized loops in noise,
    # meshing and copies). Every browser with WebGPU has it.
    -C target-feature=+atomics,+bulk-memory,+mutable-globals,+simd128
    -C link-arg=--shared-memory -C link-arg=--import-memory
    -C link-arg=--max-memory=4294967296
    # The page's own stack (workers get theirs from horizon_os): rustc's 1 MiB
    # overflowed into the heap when the loading screen's future grew, and the
    # page died with "memory access out of bounds" before the title screen.
    -C link-arg=-zstack-size=8388608
    -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size
    -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base
)
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="${flags[*]}"
# Native mods built into the game (docs/native-mods.md), by id, separated by spaces; NATIVE_MODS=
# builds the game without any. Each is a bundle in mods/<id>/ whose `[api]` package web/game links
# when its feature is on, as the launcher's generated executable does for trait mods. The launcher
# runs horizon-mixins only for bundles with injections, modules or crates: those it has to stage a
# patched copy of the source for, which this build doesn't do yet, so it refuses them.
native_mods="${NATIVE_MODS-plerocottage}"
game=(-p horizon-client --features audio)
if [ -n "$native_mods" ]; then
    for id in $native_mods; do
        bundle="mods/$id/horizon-mixin.toml"
        [ -f "$bundle" ] || { echo "native mod $id: no $bundle" >&2; exit 1; }
        python3 - "$bundle" <<'PY' || exit 1
import sys, tomllib
bundle = tomllib.load(open(sys.argv[1], "rb"))
mixins = [key for key in ("injection", "module", "crate", "post_build", "state", "bridge") if bundle.get(key)]
if "api" not in bundle or mixins:
    sys.exit(f"{sys.argv[1]}: only trait mods ([api], no {', '.join(mixins) or 'mixins'}) build into the browser game")
PY
    done
    game=(-p horizon-web-game --features "audio $native_mods")
fi
cargo=(cargo "+$nightly" -Z build-std=std,panic_abort)
# Crates the release build optimizes for size (opt-level "s"), separated by spaces; SMALL_CRATES=
# builds everything for speed.
small="${SMALL_CRATES-horizon-client horizon-client-lib horizon-protocol horizon-command horizon-items horizon-mods horizon-shaders horizon-shader-api horizon-optifine horizon-polytone horizon-perf-format horizon-client-input horizon-client-core horizon-core horizon-hooks naga toml toml_edit zip wasmparser regex-syntax regex-automata prost zopfli chrono time winit codespan-reporting pp-rs}"
if [ "$profile" = release ]; then
    for crate in $small; do
        cargo+=(--config "profile.release.package.$crate.opt-level=\"s\"")
    done
fi
"${cargo[@]}" build --target wasm32-unknown-unknown --profile "$profile" \
    "${game[@]}" --bin horizon-client --no-default-features

target_dir="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
mkdir -p "$dist/pkg"
bindgen=(wasm-bindgen --target web --no-typescript --out-dir "$dist/pkg")
# Function names (5 MB) only make stack traces readable: the release build drops them.
[ "$profile" = release ] && bindgen+=(--remove-name-section)
"${bindgen[@]}" "$target_dir/wasm32-unknown-unknown/$profile/horizon-client.wasm"
# The glue imports its snippets (`inline_js`) from folders named per crate, not
# per content: ask for each by its hash, so a cache can't hand the page an
# older one, and a changed snippet changes the glue's own hash (build.json).
python3 - "$dist/pkg/horizon-client.js" <<'PY'
import hashlib, re, sys
from pathlib import Path
glue = Path(sys.argv[1])
def stamp(m):
    rel = m.group(3)
    h = hashlib.sha256((glue.parent / rel).read_bytes()).hexdigest()[:16]
    return f"{m.group(1)}{m.group(2)}{rel}?v={h}{m.group(2)}"
# Only the import statements: the module's import object keeps the plain names.
glue.write_text(re.sub(r"""(\bfrom\s*)(['"])(\./snippets/[^'"?]+\.js)\2""", stamp, glue.read_text()))
PY
wasm="$dist/pkg/horizon-client_bg.wasm"
if [ "$profile" = release ] && [ "${WASM_OPT:-1}" != 0 ]; then
    if command -v wasm-opt > /dev/null; then
        # The features rustc's wasm32 target turns on, and the threaded build's atomics.
        wasm-opt -O2 --enable-threads --enable-bulk-memory --enable-mutable-globals --enable-simd \
            --enable-sign-ext --enable-nontrapping-float-to-int --enable-reference-types \
            --enable-multivalue "$wasm" -o "$wasm.opt"
        mv "$wasm.opt" "$wasm"
    else
        echo "wasm-opt not found: the module is left as rustc made it (install binaryen)" >&2
    fi
fi
cp web/index.html web/main.js web/audio.js web/worker.js web/sw.js "$dist/"

[ "${BUNDLE:-1}" = 0 ] && { ls -l "$dist/pkg"/*.wasm; exit 0; }
# The root the game lays its mods' content (below) over the resources in, which the page's game
# would build at every start (seconds before anything loads): the same game, built for this
# machine, builds it once here, and content.tar.gz carries what it wrote to the page
# (web/content_root.py). CONTENT_ROOT=0, or a native build that fails, leaves it to the page;
# NATIVE_TOOLCHAIN names the toolchain of the native build (the pinned nightly otherwise).
mods="${HORIZON_MODS-$(find web/mods -mindepth 1 -maxdepth 1 2>/dev/null | sort | tr '\n' ':')}"
content="$dist/fs/content.tar.gz"
replaced="$dist/fs/content-replaced.txt"
mkdir -p "$dist/fs"
if [ -n "$mods" ] && [ "${CONTENT_ROOT:-1}" != 0 ]; then
    native=(cargo "+${NATIVE_TOOLCHAIN:-$nightly}" build --profile fast -p horizon-web-game
        --bin horizon-web-content --no-default-features)
    [ -n "$native_mods" ] && native+=(--features "$native_mods")
    work="$(mktemp -d)"
    mkdir -p "$work/game/native-content" "$work/run"
    pairs=()
    IFS=: read -r -a folders <<< "$mods"
    for folder in "${folders[@]}"; do
        [ -n "$folder" ] || continue
        id="$(python3 -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1] + "/horizon-mod.toml", "rb"))["id"])' "$folder")"
        folder="$(cd "$folder" && pwd)"
        ln -s "$folder" "$work/game/native-content/$id"
        pairs+=("$id=$folder")
    done
    here="$PWD"
    # No incremental state: the fast profile's would add gigabytes to the target folder.
    if env -u CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS CARGO_INCREMENTAL=0 "${native[@]}" \
        && root="$(cd "$work/run" && HORIZON_WEB_CONTENT="$work/game" \
            "$target_dir/fast/horizon-web-content" --resources "$here/$resources" | tail -n 1)" \
        && [ -f "$root/horizon-mod-content.stamp" ]; then
        python3 web/content_root.py "$root" "$resources" "$content" "${pairs[@]}"
    else
        echo "no content root built ahead: the page's game builds its own at each start" >&2
        rm -f "$content" "$replaced"
    fi
    rm -rf "$work"
else
    rm -f "$content" "$replaced"
fi
# The resources' files that root replaces aren't downloaded up front (bundle.py): packed again
# when that list changes.
bundle=("$resources" "$dist/fs")
[ -f "$replaced" ] && bundle+=("$replaced")
replaced_sum="$(cat "$replaced" 2>/dev/null | cksum)"
if [ ! -f "$dist/fs/boot.tar.gz" ] || [ "$resources/horizon-resources.json" -nt "$dist/fs/boot.tar.gz" ] \
    || [ web/bundle.py -nt "$dist/fs/boot.tar.gz" ] \
    || [ "$replaced_sum" != "$(cat "$dist/fs/boot-replaced.cksum" 2>/dev/null)" ]; then
    python3 web/bundle.py "${bundle[@]}"
    echo "$replaced_sum" > "$dist/fs/boot-replaced.cksum"
fi
# Content mods built into this game: each folder holding a horizon-mod.toml in
# web/mods/ (or in HORIZON_MODS, separated by ':') goes beside the game as
# native-content/<id>/, where the game finds its native mods' content, always on.
python3 - "$dist/fs/mods.tar.gz" "$mods" <<'PY'
import gzip, os, sys, tarfile, tomllib
from pathlib import Path
out, folders = Path(sys.argv[1]), [Path(f).resolve() for f in sys.argv[2].split(":") if f]
if not folders:
    out.unlink(missing_ok=True)
    sys.exit()
# mtime 0 and sorted names: the same files make the same archive, and the same hash.
with open(out, "wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", compresslevel=9, mtime=0) as gz:
    with tarfile.open(fileobj=gz, mode="w", format=tarfile.GNU_FORMAT) as tar:
        for folder in folders:
            mod = tomllib.loads((folder / "horizon-mod.toml").read_text())["id"]
            files = sorted(Path(d, n) for d, _, names in os.walk(folder) for n in names)
            for path in files:
                info = tar.gettarinfo(path, arcname=f"native-content/{mod}/{path.relative_to(folder).as_posix()}")
                info.mtime, info.uid, info.gid, info.uname, info.gname = 0, 0, 0, "", ""
                with open(path, "rb") as f:
                    tar.addfile(info, f)
            print(f"mod {mod}: {len(files)} files")
PY
# The large files' content hashes: the page asks for them with these, so the
# service worker (sw.js) can keep them until they change.
python3 - "$dist" <<'PY'
import hashlib, json, sys
from pathlib import Path
dist = Path(sys.argv[1])
def digest(rel):
    h = hashlib.sha256()
    with open(dist / rel, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()[:16]
files = {rel: digest(rel) for rel in (
    "pkg/horizon-client_bg.wasm", "fs/boot.tar.gz", "pkg/horizon-client.js", "main.js", "worker.js",
    "fs/manifest.txt", "fs/mods.tar.gz", "fs/content.tar.gz") if (dist / rel).exists()}
(dist / "build.json").write_text(json.dumps(files) + "\n")
PY
ls -l "$dist/pkg"/*.wasm
echo "Serve it with: python3 web/serve.py, then open http://localhost:8080"
