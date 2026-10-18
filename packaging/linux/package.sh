#!/usr/bin/env bash
# Build and package Mortiflix Studio for Linux (<arch> is x86_64 or aarch64, or riscv64 when cross-compiling):
#
#   $DIST/mortiflix-studio-<version>-linux-<arch>.AppImage  any distro with glibc >= the build host's
#   $DIST/mortiflix-studio-<version>-linux-<arch>.AppImage.zsync  delta updates (needs zsyncmake)
#   $DIST/mortiflix-studio-<version>-linux-<arch>.deb       Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/mortiflix-studio-<version>-linux-<arch>.rpm       Fedora, openSUSE, RHEL, ...
#   $DIST/mortiflix-studio-<version>-linux-<arch>.tar.gz    plain FHS-style tree (bin/, lib/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"] [--no-mortiflix]
#
# Mortiflix Studio: every format also carries the Mortiflix bundle (the service, its own Node,
# ffmpeg, the sound engine) in lib/mortiflix-studio, made by packaging/mortiflix/bundle.sh (its
# env: MFX_SERVICE_SRC, MFX_ENGINE_BIN, NODE_VERSION, ...). --no-mortiflix leaves it out.
#
# Needs: cargo; nfpm for deb/rpm (https://nfpm.goreleaser.com); appimagetool for the AppImage
# (downloaded into $CARGO_TARGET_DIR if missing). Build on an old distro (CI: Ubuntu 22.04,
# glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate, appstreamcli,
# zsyncmake (the zsync package) for the AppImage's .zsync.
#
# Cross-compiling (CI's riscv64 job, tar only): set CROSS_ARCH (riscv64), CROSS_TARGET (the Rust
# target), CROSS_COMPILE (binutils prefix, for strip) and optionally EMULATOR (qemu-riscv64) to
# smoke-test the CLI.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=com.mortiflix.studio

SKIP_BUILD=0
MORTIFLIX=1
FORMATS="appimage deb rpm tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    --no-mortiflix) MORTIFLIX=0; shift ;;
    -h | --help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="${CROSS_ARCH:-$(uname -m)}"
case "$ARCH" in
  riscv64) DEB_ARCH=riscv64 ;;
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export MFX_STUDIO_MAINTAINER="${MFX_STUDIO_MAINTAINER:-Mortiflix Studio maintainers <mortiflix-studio@storyteller.ai>}"
BASENAME="mortiflix-studio-$VERSION-linux-$ARCH"

echo "==> Mortiflix Studio $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  if [ -n "${CROSS_TARGET:-}" ]; then
    (cd "$ROOT" && cargo build --release --locked -p mortiflix-studio -p mortiflix-studio-cli --target "$CROSS_TARGET")
  else
    (cd "$ROOT" && cargo build --release --locked -p mortiflix-studio -p mortiflix-studio-cli)
  fi
fi
BIN="$CARGO_TARGET_DIR/${CROSS_TARGET:+$CROSS_TARGET/}release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
# Mortiflix Studio installs its own command names and ids, so it never clashes with upstream Mortiflix Studio.
install -Dm755 "$BIN/mortiflix-studio" "$STAGE/usr/bin/mortiflix-studio"
install -Dm755 "$BIN/mortiflix-studio-cli" "$STAGE/usr/bin/mortiflix-studio-cli"
"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/mortiflix-studio" "$STAGE/usr/bin/mortiflix-studio-cli" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$HERE/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$MFX_STUDIO_BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
# the icons keep Mortiflix Studio's file names in the repo; installed under this app's id
find "$STAGE/usr/share/icons/hicolor" -name 'com.mortiflix.studio.*' | while read -r f; do
  to="${f%/*}/$APP_ID.${f##*.}"
  [ "$f" = "$to" ] || mv "$f" "$to" # already named for the app id since the rename
done
mkdir -p "$STAGE/usr/share/doc/mortiflix-studio"
copy_docs "$STAGE/usr/share/doc/mortiflix-studio"

# The Mortiflix bundle, found by the app at ../lib/mortiflix-studio from its binary. The folder
# always exists so nfpm's tree entry is valid without it.
mkdir -p "$STAGE/usr/lib/mortiflix-studio"
if [ "$MORTIFLIX" = 1 ]; then
  case "$ARCH" in x86_64) MFX_ARCH=x64 ;; aarch64) MFX_ARCH=arm64 ;; *) MFX_ARCH="" ;; esac
  if [ -n "$MFX_ARCH" ]; then
    "$ROOT/packaging/mortiflix/bundle.sh" --os linux --arch "$MFX_ARCH" --out "$STAGE/usr/lib/mortiflix-studio"
  else
    warn "no Mortiflix bundle for $ARCH (Node and ffmpeg builds exist for x86_64 and aarch64 only)"
  fi
fi

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz ------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  install -m755 "$HERE/install.sh" "$WORK/tar/$BASENAME/install.sh"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb / .rpm --------------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION
  export NFPM_ARCH="$DEB_ARCH"
  # nfpm expands env vars in fields like `version` and `arch`, but not in `contents[].src`.
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/MortiflixStudio.AppDir"
  cp -R "$STAGE" "$APPDIR"
  mv "$APPDIR/usr/share/doc" "$WORK/doc-unused"
  # Mortiflix Studio: AppRun offers the Mortiflix tools on a first run from a terminal, then starts the app.
  if [ -x "$APPDIR/usr/lib/mortiflix-studio/components.sh" ]; then
    install -m755 "$ROOT/packaging/mortiflix/AppRun" "$APPDIR/AppRun"
  else
    ln -s usr/bin/mortiflix-studio "$APPDIR/AppRun"
  fi
  cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$ROOT/assets/app-icon/hicolor/256x256/apps/com.mortiflix.studio.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [ -z "$TOOL" ]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [ ! -x "$TOOL" ]; then
      curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  # Absolute, because appimagetool runs in $DIST below (CARGO_TARGET_DIR or APPIMAGETOOL may be
  # relative, e.g. target/agent-<name>).
  OUT="$(cd "$DIST" && pwd)/$BASENAME.AppImage"
  APPDIR="$(cd "$APPDIR" && pwd)"
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  # A .zsync left by an earlier run would hide a missing zsyncmake and describe another file.
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate, AppImageLauncher and the like read it from the file and
  # fetch only the blocks that changed in a newer release, through the .zsync published next to
  # each AppImage on GitHub Releases. `latest` is the newest published release that is not a
  # pre-release. The releases are GITHUB_REPOSITORY's; without it the AppImage carries no update information.
  REPO="${GITHUB_REPOSITORY:-}"
  UPDATE_ARGS=()
  [ -n "$REPO" ] && UPDATE_ARGS=(-u "gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|mortiflix-studio-*-linux-$ARCH.AppImage.zsync")
  # Extract-and-run: works without FUSE (containers, CI). The output embeds the static runtime,
  # so users don't need libfuse2 either. With zsyncmake on the host (CI installs the zsync
  # package) appimagetool also writes the .zsync, into its working directory, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "${UPDATE_ARGS[@]}" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [ -s "$OUT.zsync" ]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

if [ -z "${CROSS_TARGET:-}" ]; then
  "$STAGE/usr/bin/mortiflix-studio-cli" --version
elif [ -n "${EMULATOR:-}" ]; then
  "$EMULATOR" "$STAGE/usr/bin/mortiflix-studio-cli" --version
fi
echo "==> done"
ls -lh "$DIST"
