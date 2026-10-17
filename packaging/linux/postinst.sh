#!/bin/sh
# deb/rpm post-install and post-remove: refresh the desktop, MIME and icon caches.
# Every tool is optional; missing ones are skipped.
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database /usr/share/mime || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi

# Mortiflix Studio: `mortiflix-components` (the tools Mortiflix uses), and on a fresh install with a
# terminal, the tool picker for the user who ran it (their own studio, never root's). deb passes
# `configure <old version>` (none = fresh), rpm passes 1 for a fresh install; removal cleans the link.
mfx=/usr/lib/mortiflix-studio/components.sh
if [ -x "$mfx" ]; then
  ln -sf "$mfx" /usr/bin/mortiflix-components
  fresh=0
  case "${1:-}" in configure) [ -z "${2:-}" ] && fresh=1 ;; 1) fresh=1 ;; esac
  if [ "$fresh" = 1 ]; then
    if [ -t 0 ] && [ -t 1 ] && [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ] && command -v runuser >/dev/null 2>&1; then
      user_home="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
      printf '\nMortiflix tools for %s\n' "$SUDO_USER"
      runuser -u "$SUDO_USER" -- env HOME="$user_home" "$mfx" choose || true
    else
      echo "Mortiflix tools (Blender, the 3D toolkits, Chrome, Strudel, transitions): install them in Mortiflix setup, or run 'mortiflix-components choose' as yourself."
    fi
  fi
elif [ -L /usr/bin/mortiflix-components ]; then
  rm -f /usr/bin/mortiflix-components
fi
exit 0
