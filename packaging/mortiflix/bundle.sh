#!/usr/bin/env bash
# Mortiflix FilmCraft: assemble the Mortiflix bundle every package ships beside the app.
#
#   <out>/service/   the Mortiflix service (mortiflix-oss: bin/mortiflix-service.mjs, src, pipelines, ...)
#                    with its production node_modules
#   <out>/node/      its own Node runtime (bin/node + npm/npx; node.exe on Windows)
#   <out>/bin/       ffmpeg, ffprobe and the sound engine (mortiflix-engine)
#   <out>/LICENSES/  the licences of everything above
#   <out>/BUNDLE.json  what went in (versions, sources)
#
# The app finds it beside its binary (crates/mortiflix-ui/src/service.rs `bundled_dir`):
#   Linux / AppImage / Flatpak  <prefix>/lib/mortiflix-filmcraft   (binary in <prefix>/bin)
#   Windows                     <install dir>\mortiflix            (beside filmcraft.exe)
#   macOS                       FilmCraft.app/Contents/Resources/mortiflix
#
# Usage: packaging/mortiflix/bundle.sh --os linux|windows|macos --arch x64|arm64 --out DIR
#                                      [--no-ffmpeg] [--no-engine] [--build-engine]
#
# Env:
#   MFX_SERVICE_SRC  the mortiflix-oss checkout (default: ../mortiflix-oss-filmcraft beside this repo)
#   MFX_SERVICE_REF  git ref packaged from it (default HEAD; "worktree" copies the files as they are)
#   NODE_VERSION     Node runtime (default below; checked against nodejs.org's SHASUMS256.txt)
#   MFX_ENGINE_BIN   a built mortiflix-engine for this OS/arch. Without it, --build-engine builds one
#                    with the service's own recipe (`mortiflix setup daw-engine --build`: CMake, Ninja,
#                    a C++ compiler); otherwise the bundle ships without it and setup offers the build.
#   FFMPEG_URL, FFPROBE_URL, FFMPEG_SHA256   override the ffmpeg download (FFPROBE_URL: macOS only)
#   MFX_CACHE        download cache (default: $CARGO_TARGET_DIR/mortiflix-downloads)
#
# Needs: bash, curl, tar, unzip (Windows/macOS archives), git, and npm on the host when the bundle is
# for another OS than this one (its dependencies are pure JavaScript).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

NODE_VERSION="${NODE_VERSION:-24.22.0}"
OS="" ARCH="" OUT="" FFMPEG=1 ENGINE=1 BUILD_ENGINE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --os) OS="$2"; shift 2 ;;
    --arch) ARCH="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --no-ffmpeg) FFMPEG=0; shift ;;
    --no-engine) ENGINE=0; shift ;;
    --build-engine) BUILD_ENGINE=1; shift ;;
    -h | --help) sed -n '2,33p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$OS" in linux | windows | macos) ;; *) echo "error: --os linux|windows|macos" >&2; exit 2 ;; esac
case "$ARCH" in x64 | arm64) ;; x86_64 | amd64) ARCH=x64 ;; aarch64) ARCH=arm64 ;; *) echo "error: --arch x64|arm64" >&2; exit 2 ;; esac
[ -n "$OUT" ] || { echo "error: --out DIR" >&2; exit 2; }

SRC="${MFX_SERVICE_SRC:-$(cd "$ROOT/.." && pwd)/mortiflix-oss-filmcraft}"
REF="${MFX_SERVICE_REF:-HEAD}"
[ -f "$SRC/bin/mortiflix-service.mjs" ] || { echo "error: no Mortiflix service at $SRC (set MFX_SERVICE_SRC)" >&2; exit 1; }
CACHE="${MFX_CACHE:-$CARGO_TARGET_DIR/mortiflix-downloads}"
mkdir -p "$CACHE"
EXE=""; [ "$OS" = windows ] && EXE=.exe

host_os() { case "$(uname -s)" in Linux) echo linux ;; Darwin) echo macos ;; MINGW* | MSYS* | CYGWIN*) echo windows ;; *) uname -s ;; esac; }
host_arch() { case "$(uname -m)" in x86_64 | amd64 | AMD64) echo x64 ;; aarch64 | arm64 | ARM64) echo arm64 ;; *) uname -m ;; esac; }
NATIVE=0; [ "$(host_os)" = "$OS" ] && [ "$(host_arch)" = "$ARCH" ] && NATIVE=1

# Download once into the cache; a partial file never counts.
fetch() {
  local url="$1" file="$CACHE/$2"
  if [ ! -s "$file" ]; then
    echo "  downloading $url" >&2
    curl -fsSL --retry 3 --proto '=https' -o "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  printf '%s' "$file"
}
check_sha() {
  local file="$1" want="$2" got
  got="$(sha256 "$file")"
  [ "$got" = "$want" ] || { echo "error: $(basename "$file"): sha256 $got, expected $want" >&2; rm -f "$file"; exit 1; }
}
extract() { # archive dest
  mkdir -p "$2"
  case "$1" in
    *.zip)
      if command -v unzip >/dev/null; then unzip -q -o "$1" -d "$2"
      else powershell.exe -NoProfile -Command "Expand-Archive -Force -LiteralPath '$1' -DestinationPath '$2'"; fi ;;
    *) tar -xf "$1" -C "$2" ;;
  esac
}

echo "==> Mortiflix bundle for $OS-$ARCH into $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/service" "$OUT/node" "$OUT/bin" "$OUT/LICENSES"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/mfx-bundle.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# ---- the service ---------------------------------------------------------------------------------
# What its package.json ships ("files"), plus the lock file for `npm ci`, the Claude Code plugin and
# the sound engine's source (AGPL: the engine binary ships, so does its source).
SERVICE_PATHS=(bin src web pipelines harness vendor docs plugin engine README.md LICENSE package.json package-lock.json)
if [ "$REF" = worktree ]; then
  SERVICE_SHA="$(git -C "$SRC" rev-parse HEAD 2>/dev/null || echo unknown)+worktree"
  for p in "${SERVICE_PATHS[@]}"; do
    if [ -e "$SRC/$p" ]; then cp -R "$SRC/$p" "$OUT/service/"; fi
  done
else
  SERVICE_SHA="$(git -C "$SRC" rev-parse "$REF^{commit}")"
  present=()
  for p in "${SERVICE_PATHS[@]}"; do
    if git -C "$SRC" cat-file -e "$REF:$p" 2>/dev/null; then present+=("$p"); fi
  done
  git -C "$SRC" archive --format=tar "$REF" -- "${present[@]}" | tar -x -C "$OUT/service"
fi
[ -f "$OUT/service/bin/mortiflix-service.mjs" ] || { echo "error: $SRC@$REF has no bin/mortiflix-service.mjs" >&2; exit 1; }
SERVICE_VERSION="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$OUT/service/package.json" | head -n1)"
echo "  service $SERVICE_VERSION ($SERVICE_SHA)"

# ---- Node ----------------------------------------------------------------------------------------
case "$OS" in
  linux) NODE_PKG="node-v$NODE_VERSION-linux-$ARCH.tar.xz" ;;
  macos) NODE_PKG="node-v$NODE_VERSION-darwin-$ARCH.tar.gz" ;;
  windows) NODE_PKG="node-v$NODE_VERSION-win-$ARCH.zip" ;;
esac
NODE_BASE="https://nodejs.org/dist/v$NODE_VERSION"
sums="$(fetch "$NODE_BASE/SHASUMS256.txt" "node-v$NODE_VERSION-SHASUMS256.txt")"
want="$(awk -v f="$NODE_PKG" '$2 == f { print $1 }' "$sums")"
[ -n "$want" ] || { echo "error: $NODE_PKG is not in Node's SHASUMS256.txt" >&2; exit 1; }
archive="$(fetch "$NODE_BASE/$NODE_PKG" "$NODE_PKG")"
check_sha "$archive" "$want"
extract "$archive" "$TMP/node"
node_root="$(find "$TMP/node" -mindepth 1 -maxdepth 1 -type d | head -n1)"
cp -R "$node_root/." "$OUT/node/"
# Headers, man pages and the corepack shims aren't needed to run the service.
rm -rf "$OUT/node/include" "$OUT/node/share" "$OUT/node/CHANGELOG.md" "$OUT/node/README.md"
cp "$OUT/node/LICENSE" "$OUT/LICENSES/node.txt"
if [ "$OS" = windows ]; then NODE_BIN="$OUT/node/node.exe"; else NODE_BIN="$OUT/node/bin/node"; fi
[ -f "$NODE_BIN" ] || { echo "error: no $NODE_BIN in $NODE_PKG" >&2; exit 1; }
echo "  node $NODE_VERSION"

# ---- the service's dependencies -----------------------------------------------------------------
# With the bundle's own npm when it runs here, else the host's (the dependencies are pure JS).
if [ "$NATIVE" = 1 ]; then
  if [ "$OS" = windows ]; then NPM=("$NODE_BIN" "$OUT/node/node_modules/npm/bin/npm-cli.js"); else NPM=("$NODE_BIN" "$OUT/node/lib/node_modules/npm/bin/npm-cli.js"); fi
else
  command -v npm >/dev/null || { echo "error: npm is needed on this host to bundle for $OS-$ARCH" >&2; exit 1; }
  NPM=(npm)
fi
(cd "$OUT/service" && "${NPM[@]}" ci --omit=dev --ignore-scripts --no-audit --no-fund --loglevel=error)
echo "  node_modules: $(find "$OUT/service/node_modules" -maxdepth 2 -name package.json | wc -l | tr -d ' ') packages"
cp "$OUT/service/LICENSE" "$OUT/LICENSES/mortiflix-service-AGPL-3.0.txt"

# ---- ffmpeg --------------------------------------------------------------------------------------
FFMPEG_DESC=none
if [ "$FFMPEG" = 1 ]; then
  BTBN=https://github.com/BtbN/FFmpeg-Builds/releases/download/latest
  case "$OS-$ARCH" in
    linux-x64) url="${FFMPEG_URL:-$BTBN/ffmpeg-n8.1-latest-linux64-gpl-8.1.tar.xz}" ;;
    linux-arm64) url="${FFMPEG_URL:-$BTBN/ffmpeg-n8.1-latest-linuxarm64-gpl-8.1.tar.xz}" ;;
    windows-x64) url="${FFMPEG_URL:-$BTBN/ffmpeg-n8.1-latest-win64-gpl-8.1.zip}" ;;
    windows-arm64) url="${FFMPEG_URL:-$BTBN/ffmpeg-n8.1-latest-winarm64-gpl-8.1.zip}" ;;
    macos-x64) url="${FFMPEG_URL:-https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip}" ;;
    macos-arm64) url="${FFMPEG_URL:-https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip}" ;;
  esac
  name="$OS-$ARCH-$(basename "$url")"
  archive="$(fetch "$url" "$name")"
  if [ -n "${FFMPEG_SHA256:-}" ]; then
    check_sha "$archive" "$FFMPEG_SHA256"
  elif [ "$OS" != macos ] && [ -z "${FFMPEG_URL:-}" ]; then
    # BtbN publishes checksums for each release; `latest` moves, so the sums are fetched fresh.
    curl -fsSL --retry 3 -o "$TMP/ffmpeg.sha256" "$BTBN/checksums.sha256"
    want="$(awk -v f="$(basename "$url")" '$2 == f || $2 == "*" f { print $1 }' "$TMP/ffmpeg.sha256")"
    [ -n "$want" ] || { echo "error: $(basename "$url") is not in BtbN's checksums.sha256" >&2; exit 1; }
    # `latest` moves: a cached copy of an older build is fetched again.
    if [ "$(sha256 "$archive")" != "$want" ]; then rm -f "$archive" && archive="$(fetch "$url" "$name")"; fi
    check_sha "$archive" "$want"
  else
    warn "ffmpeg for $OS-$ARCH is not checksum-pinned (set FFMPEG_SHA256)"
  fi
  extract "$archive" "$TMP/ffmpeg"
  if [ "$OS" = macos ]; then
    pa="$(fetch "${FFPROBE_URL:-${url%/ffmpeg.zip}/ffprobe.zip}" "$OS-$ARCH-ffprobe.zip")"
    extract "$pa" "$TMP/ffmpeg"
  fi
  for tool in ffmpeg ffprobe; do
    f="$(find "$TMP/ffmpeg" -type f -name "$tool$EXE" | head -n1)"
    [ -n "$f" ] || { echo "error: no $tool$EXE in $(basename "$url")" >&2; exit 1; }
    install -m755 "$f" "$OUT/bin/$tool$EXE"
  done
  lic="$(find "$TMP/ffmpeg" -maxdepth 3 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' \) | head -n1)"
  {
    echo "ffmpeg and ffprobe: GPL-3.0-or-later builds (with libx264 and other GPL libraries)."
    echo "Downloaded from $url"
    echo "Source code: https://ffmpeg.org/download.html and https://github.com/BtbN/FFmpeg-Builds"
    echo
    if [ -n "$lic" ]; then cat "$lic"; fi
  } >"$OUT/LICENSES/ffmpeg.txt"
  FFMPEG_DESC="$url"
  if [ "$NATIVE" = 1 ]; then echo "  $("$OUT/bin/ffmpeg$EXE" -hide_banner -version | head -n1)"; fi
fi

# ---- the sound engine ----------------------------------------------------------------------------
ENGINE_DESC=none
if [ "$ENGINE" = 1 ]; then
  bin="${MFX_ENGINE_BIN:-}"
  if [ -z "$bin" ] && [ "$BUILD_ENGINE" = 1 ]; then
    [ "$NATIVE" = 1 ] || { echo "error: --build-engine only builds for this machine's OS" >&2; exit 1; }
    # The service's own recipe: pinned Tracktion Engine + JUCE commits (engine/deps.json), CMake + Ninja.
    studio="$TMP/engine-studio"
    (cd "$SRC" && MORTIFLIX_STUDIO="$studio" "$NODE_BIN" bin/mortiflix setup daw-engine --build)
    bin="$studio/tools/daw-engine/mortiflix-engine$EXE"
  fi
  if [ -n "$bin" ]; then
    [ -f "$bin" ] || { echo "error: MFX_ENGINE_BIN=$bin does not exist" >&2; exit 1; }
    install -m755 "$bin" "$OUT/bin/mortiflix-engine$EXE"
    if [ "$NATIVE" = 1 ]; then ENGINE_DESC="$("$OUT/bin/mortiflix-engine$EXE" --version | tail -n1)"; else ENGINE_DESC="\"$(basename "$bin")\""; fi
    {
      echo "mortiflix-engine: AGPL-3.0-only (Mortiflix), built with JUCE (AGPL-3.0) and Tracktion Engine (GPL-3.0-or-later)."
      echo "Its source is in service/engine/ (CMakeLists.txt, src/, patches/); the exact upstream commits are in"
      echo "service/engine/deps.json. The AGPL-3.0 text is in mortiflix-service-AGPL-3.0.txt."
    } >"$OUT/LICENSES/mortiflix-engine.txt"
    echo "  engine $ENGINE_DESC"
  else
    warn "no sound engine in the bundle (set MFX_ENGINE_BIN or pass --build-engine); setup offers to build it"
  fi
fi

# ---- the components step -------------------------------------------------------------------------
# The installers' tool picker (Blender, the 3D toolkits, Chrome, Strudel, transitions, uv, ComfyUI): the service's
# own installers, run with the bundled Node. Windows runs the .mjs with node.exe directly.
if [ "$OS" != windows ]; then install -m755 "$ROOT/packaging/mortiflix/components.sh" "$OUT/components.sh"; fi
[ -f "$OUT/service/bin/mortiflix-components.mjs" ] || warn "this service has no bin/mortiflix-components.mjs: the installers can't offer the tools"

# ---- what went in --------------------------------------------------------------------------------
cat >"$OUT/BUNDLE.json" <<EOF
{
  "os": "$OS",
  "arch": "$ARCH",
  "service": { "version": "$SERVICE_VERSION", "commit": "$SERVICE_SHA" },
  "node": "$NODE_VERSION",
  "ffmpeg": "$FFMPEG_DESC",
  "engine": $( [ "$ENGINE_DESC" = none ] && echo '"none"' || echo "$ENGINE_DESC" )
}
EOF
cat >"$OUT/LICENSES/README.txt" <<'EOF'
The Mortiflix bundle is a separate program that Mortiflix FilmCraft starts as a child process.
Each part keeps its own licence:
  service/    the Mortiflix service: AGPL-3.0-only (mortiflix-service-AGPL-3.0.txt); its source is the
              service/ folder itself
  node/       Node.js: MIT and the bundled licences in node.txt
  bin/ffmpeg, bin/ffprobe: GPL-3.0-or-later (ffmpeg.txt)
  bin/mortiflix-engine:    AGPL-3.0-only with JUCE and Tracktion Engine (mortiflix-engine.txt)
EOF

# Smoke test when the bundle runs here: the service answers hello over stdin/stdout.
if [ "$NATIVE" = 1 ]; then
  reply="$(cd "$OUT/service" && printf '%s\n' '{"id":1,"type":"hello","params":{"client":"bundle.sh","protocol":1}}' \
    | MORTIFLIX_STUDIO="$TMP/studio" timeout 30 "$NODE_BIN" bin/mortiflix-service.mjs 2>/dev/null | head -n1 || true)"
  case "$reply" in
    *'"ok":true'*) echo "  service hello: ok" ;;
    *) echo "error: the bundled service did not answer hello: $reply" >&2; exit 1 ;;
  esac
fi
echo "  size: $(du -sh "$OUT" | cut -f1)"
echo "==> bundle done"
