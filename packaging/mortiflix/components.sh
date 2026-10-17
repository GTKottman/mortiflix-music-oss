#!/bin/sh
# Mortiflix components: the tools Mortiflix uses (Blender + the 3D toolkits, headless Chrome, Strudel, the
# transitions library, uv, ComfyUI), installed for the user who runs this into their studio (~/Mortiflix, or
# MORTIFLIX_STUDIO), never system-wide. Runs the service's own installers with the bundled Node.
#
#   components.sh list [--json] | install <default|all|none|id,id,...> | choose
#
# Shipped in the Mortiflix bundle (packaging/mortiflix/bundle.sh); installers call it, and so can you later.
here="$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd -P)"
PATH="$here/bin:$here/node/bin:$PATH"
export PATH
exec "$here/node/bin/node" "$here/service/bin/mortiflix-components.mjs" "$@"
