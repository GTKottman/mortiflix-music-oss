#!/usr/bin/env bash
# Build, sign and (optionally) notarize the macOS release artifacts:
#
#   $DIST/mortiflix-studio-<version>-macos-<arch>.dmg  Mortiflix Studio.app on a drag-to-Applications DMG
#   $DIST/mortiflix-studio-cli-<version>-macos-<arch>.zip      the headless CLI
#   $DIST/mortiflix-studio-<version>-macos-<arch>.pkg          Mortiflix Studio: the app + "Mortiflix tools" choices
#                                                       (Blender, 3D toolkits, Chrome, Strudel, transitions, uv
#                                                       ticked; ComfyUI not), downloaded during the install.
#                                                       Signed with MACOS_INSTALLER_IDENTITY ("Developer ID
#                                                       Installer") when set; notarized like the DMG.
#
# Usage: packaging/macos/package.sh [--arch universal|aarch64|x86_64] [--skip-build] [--no-mortiflix]
#
# Mortiflix Studio: Mortiflix Studio.app also carries the Mortiflix bundle (the service, its own Node,
# ffmpeg, the sound engine) in Contents/Resources/mortiflix, made by packaging/mortiflix/bundle.sh
# (env: MFX_SERVICE_SRC, NODE_VERSION, FFMPEG_SHA256, ...; MFX_ENGINE_BIN = the arm64 or only
# engine, MFX_ENGINE_BIN_X64 = the x86_64 one for a universal build). Universal: each tool is
# lipo'd from its arm64 and x86_64 builds. Every Mach-O in it is signed with the hardened runtime
# (Node with packaging/mortiflix/node-entitlements.plist), so it notarizes with the app.
#
# Signing (env):
#   MACOS_SIGN_IDENTITY   codesign identity (name or SHA-1). Default "-" = ad-hoc (local testing;
#                         Gatekeeper will reject the result on other Macs). In CI, import-cert.sh sets it.
#   MACOS_KEYCHAIN        keychain holding the identity (optional)
# Notarization (env; all three needed, and a real identity):
#   APPLE_ID, APPLE_PASSWORD (app-specific password), APPLE_TEAM_ID
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/macos"

ARCH=universal
SKIP_BUILD=0
MORTIFLIX=1
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    --no-mortiflix) MORTIFLIX=0; shift ;;
    -h | --help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  aarch64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "unknown --arch $ARCH" >&2; exit 2 ;;
esac

# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-package"
APP="$WORK/Mortiflix Studio.app"
DMG="$DIST/mortiflix-studio-$VERSION-macos-$ARCH.dmg"
CLI_ZIP="$DIST/mortiflix-studio-cli-$VERSION-macos-$ARCH.zip"
PKG="$DIST/mortiflix-studio-$VERSION-macos-$ARCH.pkg"

NOTARIZE=0
if [ "$IDENTITY" = "-" ]; then
  warn "macOS: ad-hoc signing (no MACOS_SIGN_IDENTITY); artifacts are not notarized"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  NOTARIZE=1
else
  warn "macOS: APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID incomplete; signed but not notarized"
fi

echo "==> Mortiflix Studio $VERSION for macOS ($ARCH), identity: $IDENTITY, notarize: $NOTARIZE"

# ---- build -------------------------------------------------------------------------------------
if [ "$SKIP_BUILD" = 0 ]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked -p mortiflix-studio -p mortiflix-studio-cli "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
for bin in mortiflix-studio mortiflix-studio-cli; do
  inputs=()
  for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/$bin"); done
  lipo -create -output "$WORK/bin/$bin" "${inputs[@]}"
  lipo -info "$WORK/bin/$bin"
done

# ---- signing helpers ---------------------------------------------------------------------------
sign() {
  # Hardened runtime + secure timestamp for a real identity; ad-hoc can't be timestamped.
  local ts=(--timestamp)
  [ "$IDENTITY" = "-" ] && ts=(--timestamp=none)
  local kc=()
  [ -n "${MACOS_KEYCHAIN:-}" ] && kc=(--keychain "$MACOS_KEYCHAIN")
  codesign --force --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" "$@"
}

notarize() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file") (this can take a few minutes)"
  out="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait --timeout 1h --output-format json)" || true
  echo "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [ "$status" != "Accepted" ]; then
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true
    fi
    echo "error: notarization of $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
}

# ---- Mortiflix Studio.app ----------------------------------------------------------------------------
echo "==> assembling $APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# Executable and icon carry the display name (CFBundleExecutable / CFBundleIconFile).
cp "$WORK/bin/mortiflix-studio" "$APP/Contents/MacOS/Mortiflix Studio"
cp "$ROOT/assets/app-icon/mortiflix-studio.icns" "$APP/Contents/Resources/Mortiflix Studio.icns"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" \
  -e "s/@BUILD_SHA@/${MFX_STUDIO_BUILD_SHA:-unknown}/g" \
  "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
# licences of the embedded craft-fonts fonts (builds with CRAFT_FONTS_DIR)
copy_font_licences "$APP/Contents/Resources"

# ---- Mortiflix bundle --------------------------------------------------------------------------
# Found by the app at ../Resources/mortiflix from Contents/MacOS (crates/mortiflix-ui/src/service.rs).
MFX="$APP/Contents/Resources/mortiflix"
if [ "$MORTIFLIX" = 1 ]; then
  BUNDLE_SH="$ROOT/packaging/mortiflix/bundle.sh"
  case "$ARCH" in
    aarch64) "$BUNDLE_SH" --os macos --arch arm64 --out "$MFX" ;;
    x86_64) "$BUNDLE_SH" --os macos --arch x64 --out "$MFX" ;;
    universal)
      "$BUNDLE_SH" --os macos --arch arm64 --out "$MFX"
      MFX_ENGINE_BIN="${MFX_ENGINE_BIN_X64:-}" "$BUNDLE_SH" --os macos --arch x64 --out "$WORK/mortiflix-x64"
      while IFS= read -r f; do
        twin="$WORK/mortiflix-x64/${f#"$MFX"/}"
        if [ -f "$twin" ] && lipo -info "$f" >/dev/null 2>&1; then
          lipo -create -output "$f.universal" "$f" "$twin"
          mv "$f.universal" "$f"
          lipo -info "$f"
        elif lipo -info "$f" >/dev/null 2>&1; then
          warn "$(basename "$f") has no x86_64 build: it runs on Apple silicon only"
        fi
      done < <(find "$MFX" -type f -perm -u+x)
      ;;
  esac
fi

# Sign inside-out: nested code first, then the bundle itself (no --deep on the final signature).
# Nested code: the Mortiflix bundle's tools (Node needs its JIT entitlements), then the main executable.
if [ -d "$MFX" ]; then
  while IFS= read -r f; do
    if file -b "$f" | grep -q 'Mach-O'; then
      case "$f" in
        */node/bin/node) sign --options runtime --entitlements "$ROOT/packaging/mortiflix/node-entitlements.plist" "$f" ;;
        *) sign --options runtime "$f" ;;
      esac
    fi
  done < <(find "$MFX" -type f)
fi
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/Mortiflix Studio"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

if [ "$NOTARIZE" = 1 ]; then
  ditto -c -k --keepParent "$APP" "$WORK/Mortiflix Studio-notarize.zip"
  notarize "$WORK/Mortiflix Studio-notarize.zip"
  xcrun stapler staple "$APP"
  xcrun stapler validate "$APP"
  spctl --assess --type execute -vvv "$APP"
fi

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> building $DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/Mortiflix Studio.app"
ln -s /Applications "$STAGE/Applications"
# Finder window layout: background, icon size and positions (packaging/macos/dmg/README.md).
mkdir -p "$STAGE/.background"
cp "$HERE/dmg/background.tiff" "$STAGE/.background/background.tiff"
cp "$HERE/dmg/dmg-layout.DS_Store" "$STAGE/.DS_Store"
rm -f "$DMG" "$WORK/raw.dmg"
# makehybrid + convert builds the image without attaching a device, unlike `create -srcfolder`,
# which is flaky on CI runners ("Resource busy") and hangs in sandboxed sessions.
# The volume name has no version: .DS_Store finds the background through an alias that includes it.
hdiutil makehybrid -hfs -hfs-volume-name "Mortiflix Studio" -hfs-openfolder "$STAGE" -o "$WORK/raw.dmg" "$STAGE"
hdiutil convert "$WORK/raw.dmg" -format UDZO -imagekey zlib-level=9 -o "$DMG"
rm -f "$WORK/raw.dmg"
sign "$DMG"
codesign --verify --strict --verbose=2 "$DMG"
if [ "$NOTARIZE" = 1 ]; then
  notarize "$DMG"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
  spctl --assess --type open --context context:primary-signature -vvv "$DMG"
fi

# ---- .pkg with the Mortiflix tools as choices ----------------------------------------------------
# The app, plus one payload-free package per tool whose postinstall runs the bundle's own installer for it
# (packaging/mortiflix/macos-tool-postinstall.sh) as the logged-in user. "Customize" shows the choices.
if [ -d "$MFX" ]; then
  echo "==> building $PKG"
  PW="$WORK/pkg"
  rm -rf "$PW" && mkdir -p "$PW/parts"
  pkgbuild --component "$APP" --install-location /Applications --identifier com.mortiflix.studio --version "$SHORT_VERSION" "$PW/parts/app.pkg"
  # id|title|ticked|description
  TOOLS=(
    "blender|Blender (~350 MB)|true|3D shots: the newest official Blender, checked against its sha256 list (yours is reused if you have one)."
    "blender-addons|The 3D toolkits (~15 MB)|true|MoBlend, Nova FX, Camera, Animate, Math, Circuits and Camera Flight, in the studio's own Blender profile."
    "chrome|Headless Chrome (~100 MB)|true|Renders the music and the Remotion shots (an installed Chrome is reused)."
    "strudel|Strudel (~20 MB)|true|The music step: it writes and renders the score."
    "transitions|Transitions library (~5 MB)|true|Every cut: 50 designed transitions the AI picks from."
    "uv|uv, for Python tools (~40 MB)|true|Runs the Python tools (browser-harness, ComfyUI) with their own Python."
    "comfyui|ComfyUI + Qwen3-TTS (~10 GB)|false|Local narration. Needs an NVIDIA graphics card, so it does nothing on a Mac today: use ElevenLabs or your own voice."
  )
  lines="" choices="" refs=""
  for t in "${TOOLS[@]}"; do
    IFS='|' read -r id title on desc <<<"$t"
    mkdir -p "$PW/scripts-$id"
    sed "s/@TOOL_ID@/$id/" "$ROOT/packaging/mortiflix/macos-tool-postinstall.sh" >"$PW/scripts-$id/postinstall"
    chmod 755 "$PW/scripts-$id/postinstall"
    pkgbuild --nopayload --scripts "$PW/scripts-$id" --identifier "com.mortiflix.studio.tools.$id" --version "$SHORT_VERSION" "$PW/parts/tool-$id.pkg"
    lines="$lines<line choice=\"$id\"/>"
    choices="$choices<choice id=\"$id\" title=\"$title\" description=\"$desc\" start_selected=\"$on\"><pkg-ref id=\"com.mortiflix.studio.tools.$id\"/></choice>"
    refs="$refs<pkg-ref id=\"com.mortiflix.studio.tools.$id\" version=\"$SHORT_VERSION\">tool-$id.pkg</pkg-ref>"
  done
  cat >"$PW/distribution.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
  <title>Mortiflix Studio</title>
  <options customize="always" require-scripts="false" hostArchitectures="arm64,x86_64"/>
  <choices-outline><line choice="app"/><line choice="tools">$lines</line></choices-outline>
  <choice id="app" title="Mortiflix Studio" enabled="false" selected="true"><pkg-ref id="com.mortiflix.studio"/></choice>
  <choice id="tools" title="Mortiflix tools (downloaded now)" description="The tools Mortiflix uses, downloaded into your own Mortiflix studio. Skip any: Mortiflix setup can install them later. Log: ~/Library/Logs/Mortiflix tools.log"/>
  $choices
  <pkg-ref id="com.mortiflix.studio" version="$SHORT_VERSION">app.pkg</pkg-ref>
  $refs
</installer-gui-script>
XML
  rm -f "$PKG"
  productbuild --distribution "$PW/distribution.xml" --package-path "$PW/parts" "$PW/unsigned.pkg"
  if [ -n "${MACOS_INSTALLER_IDENTITY:-}" ]; then
    kc=()
    [ -n "${MACOS_KEYCHAIN:-}" ] && kc=(--keychain "$MACOS_KEYCHAIN")
    productsign --sign "$MACOS_INSTALLER_IDENTITY" ${kc[@]+"${kc[@]}"} "$PW/unsigned.pkg" "$PKG"
    if [ "$NOTARIZE" = 1 ]; then notarize "$PKG"; xcrun stapler staple "$PKG"; fi
  else
    warn "macOS: no MACOS_INSTALLER_IDENTITY; the .pkg is unsigned (Gatekeeper warns on other Macs)"
    mv "$PW/unsigned.pkg" "$PKG"
  fi
fi

# ---- CLI ---------------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/mortiflix-studio-cli-$VERSION-macos-$ARCH"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/mortiflix-studio-cli" "$CLI_DIR/"
copy_docs "$CLI_DIR"
sign --options runtime "$CLI_DIR/mortiflix-studio-cli"
codesign --verify --strict --verbose=2 "$CLI_DIR/mortiflix-studio-cli"
rm -f "$CLI_ZIP"
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"
# A bare Mach-O can't carry a stapled ticket; Gatekeeper looks the notarization up online.
if [ "$NOTARIZE" = 1 ]; then notarize "$CLI_ZIP"; fi

"$WORK/bin/mortiflix-studio-cli" --version
echo "==> done"
ls -lh "$DMG" "$CLI_ZIP"
if [ -f "$PKG" ]; then ls -lh "$PKG"; fi
