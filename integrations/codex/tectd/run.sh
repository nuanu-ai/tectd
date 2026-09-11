#!/bin/sh
set -eu
: "${TECT_SOCKET:?TECT_SOCKET must name the private daemon socket}"
: "${TECT_HOST_CONFIG:?TECT_HOST_CONFIG must name the private enrollment file}"
: "${TECT_WORKSPACE_KEY:?TECT_WORKSPACE_KEY must be an explicit logical key}"
plugin_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
exec "$plugin_root/bin/tectd-mcp"
