#!/usr/bin/env bash
# Install the extracted Linux tarball. No sudo is needed for the default prefix.
set -euo pipefail

usage() {
  echo 'Usage: ./install.sh [--prefix /absolute/path] [--components default|all|none|id,id,...]'
  echo 'Installs Mortiflix Studio (mortiflix-studio) and its desktop integration into ~/.local by default.'
  echo 'For all users: sudo ./install.sh --prefix /usr/local'
  echo 'Then the Mortiflix tools (Blender + 3D toolkits, Chrome, Strudel, transitions, uv; ComfyUI off) are offered:'
  echo 'asked one by one in a terminal, or chosen with --components (none = skip; install them later in setup).'
}
fail() { echo "error: $*" >&2; exit 1; }
prefix="${HOME:?HOME is not set}/.local"
components="${MORTIFLIX_COMPONENTS:-}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      [ "$#" -ge 2 ] || fail '--prefix needs an absolute path'
      prefix="$2"; shift 2 ;;
    --components)
      [ "$#" -ge 2 ] || fail '--components needs default, all, none or a list'
      components="$2"; shift 2 ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; fail "unknown argument: $1" ;;
  esac
done
case "$prefix" in /*) ;; *) fail '--prefix must be an absolute path' ;; esac
prefix="$(realpath -m -- "$prefix")"
# Desktop Exec paths cannot contain '='; launchers also misinterpret '%' in executable names.
[[ "$prefix" != *'='* && "$prefix" != *'%'* && ! "$prefix" =~ [[:cntrl:]] ]] || fail 'prefix cannot contain =, % or control characters'
[ "$prefix" != / ] || fail 'use /usr/local or a user directory, not /'
source_dir="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)"
case "$prefix/" in "$source_dir/"*) fail 'choose an installation prefix outside the extracted tarball' ;; esac
app_id=com.mortiflix.studio
for binary in mortiflix-studio mortiflix-studio-cli; do
  [ -f "$source_dir/bin/$binary" ] || fail "missing bin/$binary; extract the complete tarball first"
done
[ -f "$source_dir/share/applications/$app_id.desktop" ] || fail 'missing desktop entry; extract the complete tarball first'

mkdir -p -- "$prefix/bin" "$prefix/share"
install -m755 -- "$source_dir/bin/mortiflix-studio" "$prefix/bin/mortiflix-studio"
install -m755 -- "$source_dir/bin/mortiflix-studio-cli" "$prefix/bin/mortiflix-studio-cli"
cp -R -- "$source_dir/share/." "$prefix/share/"
# Mortiflix Studio: the service, its Node, ffmpeg and the sound engine (found from bin/ as
# ../lib/mortiflix-studio). A fresh copy, so files from an older version never linger.
if [ -d "$source_dir/lib/mortiflix-studio" ]; then
  rm -rf -- "$prefix/lib/mortiflix-studio"
  mkdir -p -- "$prefix/lib"
  cp -R -- "$source_dir/lib/mortiflix-studio" "$prefix/lib/mortiflix-studio"
  if [ -x "$prefix/lib/mortiflix-studio/components.sh" ]; then
    ln -sf -- "../lib/mortiflix-studio/components.sh" "$prefix/bin/mortiflix-components"
  fi
fi

# Use an absolute path so launching from the application menu does not depend on PATH.
# Escape the Exec argument, then the desktop entry's string value (two separate layers).
# https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html
exec_path="$(printf '%s' "$prefix/bin/mortiflix-studio" | sed 's/[\\"`$]/\\&/g; s/\\/\\\\/g')"
try_path="$(printf '%s' "$prefix/bin/mortiflix-studio" | sed 's/\\/\\\\/g')"
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in
    Exec=*) printf 'Exec="%s" %%F\n' "$exec_path" ;;
    TryExec=*) printf 'TryExec=%s\n' "$try_path" ;;
    *) printf '%s\n' "$line" ;;
  esac
done <"$source_dir/share/applications/$app_id.desktop" >"$prefix/share/applications/$app_id.desktop"

# Cache tools are optional; installing binaries must also work on minimal systems.
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q "$prefix/share/applications" || echo 'warning: desktop cache refresh failed' >&2
fi
if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database "$prefix/share/mime" || echo 'warning: MIME cache refresh failed' >&2
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f "$prefix/share/icons/hicolor" || echo 'warning: icon cache refresh failed' >&2
fi
printf 'Installed Mortiflix Studio in %s\nLaunch it from the application menu or run "%s/bin/mortiflix-studio".\n' "$prefix" "$prefix"
case ":${PATH:-}:" in
  *":$prefix/bin:"*) ;;
  *) printf 'For terminal commands, add %s/bin to your PATH.\n' "$prefix" ;;
esac

# ---- Mortiflix components: the tools Mortiflix uses, for the user (never system-wide) ----------------
# Downloaded now with progress, each checked; one that fails or is skipped never fails this install.
mfx_components="$prefix/lib/mortiflix-studio/components.sh"
if [ -x "$mfx_components" ]; then
  # Under sudo the tools belong to the user who ran it, in their own studio.
  as_user() {
    if [ "$(id -u)" = 0 ] && [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then sudo -u "$SUDO_USER" -H -- "$@"; else "$@"; fi
  }
  if [ -n "$components" ]; then
    as_user "$mfx_components" install "$components" || echo 'warning: some Mortiflix tools did not install; Mortiflix setup can install them later' >&2
  elif [ -t 0 ] && [ -t 1 ]; then
    printf '\nMortiflix tools\n'
    as_user "$mfx_components" choose || echo 'warning: some Mortiflix tools did not install; Mortiflix setup can install them later' >&2
  else
    printf 'Mortiflix tools were not installed (no terminal to ask in). Install them in Mortiflix setup, or run:\n  %s choose\n' "$prefix/bin/mortiflix-components"
  fi
fi
