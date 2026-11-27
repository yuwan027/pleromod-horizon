#!/usr/bin/env bash
# Builds Horizon for the browser into web/dist: the game as WebAssembly, the
# page, and the prepared resources packed for download.
#
#   web/build.sh            the release build, what is deployed: thin LTO, the crates in SMALL_CRATES
#                           at opt-level "s", then wasm-opt -O2 (binaryen) when installed
#   PROFILE=fast web/build.sh  optimized, no LTO or wasm-opt, incremental: quick to rebuild while
#                           working (a larger, slower module with function names in its stacks)
#   WASM_OPT=0|1            skip wasm-opt, or run it on another profile too (only release runs it)
#   CONTENT_ROOT=0          no content root: the page's game builds its own at each start
#   CONTENT_CACHE=0         build the content root again even when .local/web-cache has it
#   DIST=<folder>           build into another folder (web/dist otherwise)
#   BUNDLE=0                only the game and the page, not the resources
#
# Every mode serves the same files. It prints how long each phase took at the end.
#
# The content root (below) comes from a native build of the game, which recompiles much of
# it after an engine change: it is kept in .local/web-cache/content/<key>, and reused while
# the key is the same. The key covers the mods (web/mods), the prepared resources'
# horizon-resources.json, the native mods built in, and the sources that build the root: the
# crates horizon-mods, -items, -protocol, -world and -core, horizon-client's mod_content*,
# content_identity, mod_api and game/{mods,native_hooks}.rs, the native mods' folders, web/game,
# web/content_root.py, Cargo.toml and Cargo.lock. A change elsewhere that alters the root (code
# those files call in another crate) needs CONTENT_CACHE=0.
#
# Threads are Web Workers on shared memory, which std only supports when it is
# rebuilt with atomics: that build needs the pinned nightly below with its
# rust-src (rustup component add rust-src --toolchain <it>).
# Needs: rustup target add wasm32-unknown-unknown, and the wasm-bindgen CLI of
# the version in Cargo.lock (cargo install wasm-bindgen-cli --version <it>); for
# the release build, wasm-opt (binaryen's package, e.g. pacman -S binaryen).
set -euo pipefail
cd "$(dirname "$0")/.."

# Each phase's time in whole seconds, printed when the build ends (or fails).
timings=()
phase_start=$SECONDS
phase() {
    timings+=("$(printf '  %-14s %4d s' "$1" $((SECONDS - phase_start)))")
    phase_start=$SECONDS
}
report() {
    [ ${#timings[@]} -gt 0 ] || return 0
    echo "phases:"
    printf '%s\n' "${timings[@]}"
    printf '  %-14s %4d s\n' total "$SECONDS"
}
trap report EXIT

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
phase "wasm build"

wasm_opt="${WASM_OPT:-$([ "$profile" = release ] && echo 1 || echo 0)}"
if [ "$wasm_opt" != 0 ] && ! command -v wasm-opt > /dev/null; then
    echo "wasm-opt not found: the module is left as rustc made it (install binaryen)" >&2
    wasm_opt=0
fi
target_dir="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
mkdir -p "$dist/pkg"
bindgen=(wasm-bindgen --target web --no-typescript --out-dir "$dist/pkg")
# Function names (5 MB) only make stack traces readable: the release build drops them. wasm-opt
# drops them itself (it writes none without -g), after naming the functions in its symbol map.
[ "$profile" = release ] && [ "$wasm_opt" = 0 ] && bindgen+=(--remove-name-section)
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
phase "wasm-bindgen"
wasm="$dist/pkg/horizon-client_bg.wasm"
rm -f "$dist/horizon-client.symbols"
if [ "$wasm_opt" != 0 ]; then
    # The features rustc's wasm32 target turns on, and the threaded build's atomics.
    wasm-opt -O2 --enable-threads --enable-bulk-memory --enable-mutable-globals --enable-simd \
        --enable-sign-ext --enable-nontrapping-float-to-int --enable-reference-types \
        --enable-multivalue "$wasm" -o "$wasm.opt" \
        --symbolmap --pass-arg="symbolmap@$dist/horizon-client.symbols"
    # The symbol map names the functions a panic's stack gives by number
    # (wasm-function[n]); it stays beside the build, out of what is served.
    mv "$wasm.opt" "$wasm"
    phase "wasm-opt"
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
cache=""
# What the root is built from (the key's files: the top of this file).
content_key() {
    python3 - "$resources/horizon-resources.json" "content cache 1 ${NATIVE_TOOLCHAIN:-$nightly} $native_mods" "$mods" \
        $(for id in $native_mods; do echo "mods/$id"; done) <<'PY'
import hashlib, os, sys
from pathlib import Path
manifest, setup, mods, natives = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4:]
h = hashlib.sha256()
def add(label, data):
    h.update(f"{label}\0{len(data)}\0".encode())
    h.update(data)
def tree(folder, keep=lambda p: True):
    folder = Path(folder)
    for d, dirs, names in os.walk(folder):
        dirs[:] = sorted(x for x in dirs if x not in ("target", "__pycache__", ".git"))
        for n in sorted(names):
            p = Path(d, n)
            if keep(p):
                add(f"{folder.name}/{p.relative_to(folder).as_posix()}", p.read_bytes())
add("setup", setup.encode())
add("resources", manifest.read_bytes() if manifest.exists() else b"")
for folder in (f for f in mods.split(":") if f):
    tree(folder)
for crate in ("horizon-mods", "horizon-items", "horizon-protocol", "horizon-world", "horizon-core"):
    add(crate, Path(f"crates/{crate}/Cargo.toml").read_bytes())
    tree(f"crates/{crate}/src")
    if Path(f"crates/{crate}/build.rs").exists():
        add(crate + " build.rs", Path(f"crates/{crate}/build.rs").read_bytes())
client = Path("crates/horizon-client/src")
for p in sorted(client.glob("mod_content*.rs")) + [client / f for f in (
        "content_identity.rs", "mod_api.rs", "game/mods.rs", "game/native_hooks.rs")]:
    add(p.as_posix(), p.read_bytes())
for folder in natives + ["web/game"]:
    tree(folder)
for f in ("web/content_root.py", "Cargo.toml", "Cargo.lock"):
    add(f, Path(f).read_bytes())
print(h.hexdigest()[:24])
PY
}
if [ -n "$mods" ] && [ "${CONTENT_ROOT:-1}" != 0 ]; then
    key="$(content_key)"
    cache=".local/web-cache/content/$key"
fi
phase "content key"
if [ -n "$cache" ] && [ "${CONTENT_CACHE:-1}" != 0 ] && [ -f "$cache/content.tar.gz" ]; then
    echo "content root: reused from $cache"
    cp "$cache/content.tar.gz" "$content"
    if [ -f "$cache/content-replaced.txt" ]; then cp "$cache/content-replaced.txt" "$replaced"; else rm -f "$replaced"; fi
    touch "$cache"
elif [ -n "$cache" ]; then
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
        # Kept for the next build with the same key, unless its files changed while it ran;
        # the four last used stay.
        if [ "$(content_key)" = "$key" ]; then
            rm -rf "$cache" "$cache.new"
            mkdir -p "$cache.new"
            cp "$content" "$cache.new/"
            [ -f "$replaced" ] && cp "$replaced" "$cache.new/"
            mv "$cache.new" "$cache"
            ls -dt "$(dirname "$cache")"/*/ | tail -n +5 | while read -r old; do rm -rf "$old"; done
        else
            echo "content root: not kept, its sources changed during the build" >&2
        fi
    else
        echo "no content root built ahead: the page's game builds its own at each start" >&2
        rm -f "$content" "$replaced"
    fi
    rm -rf "$work"
else
    rm -f "$content" "$replaced"
fi
phase "content root"
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
phase "bundle"
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
phase "mods, hashes"
ls -l "$dist/pkg"/*.wasm
echo "Serve it with: python3 web/serve.py, then open http://localhost:8080"
