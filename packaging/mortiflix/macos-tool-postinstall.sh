#!/bin/sh
# Mortiflix Studio .pkg: one of the "Mortiflix tools" choices (packaging/macos/package.sh fills in TOOL_ID).
# Installer runs this as root after Mortiflix Studio.app is in /Applications; the tool goes to the logged-in user's own
# studio (~/Mortiflix), installed by the bundle's own installers as that user. Never fails the install.
TOOL_ID="@TOOL_ID@"
app="${MFX_APP:-/Applications/Mortiflix Studio.app}"
mfx="$app/Contents/Resources/mortiflix"
user="$(stat -f%Su /dev/console 2>/dev/null)"
if [ -z "$user" ] || [ "$user" = root ] || [ ! -x "$mfx/node/bin/node" ]; then exit 0; fi
home="$(dscl . -read "/Users/$user" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
[ -n "$home" ] || exit 0
log="$home/Library/Logs/Mortiflix tools.log"
sudo -u "$user" -H env HOME="$home" PATH="$mfx/bin:$mfx/node/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  "$mfx/node/bin/node" "$mfx/service/bin/mortiflix-components.mjs" install "$TOOL_ID" >>"$log" 2>&1 || true
chown "$user" "$log" 2>/dev/null || true
exit 0
