#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  printf 'usage: %s WADDLE_BINARY\n' "$0" >&2
  exit 2
fi

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
binary=$(realpath -- "$1")
smoke_dir=$(mktemp -d /tmp/waddle-file-manager-smoke.XXXXXX)
trap 'rm -rf -- "$smoke_dir"' EXIT
mkdir -p "$smoke_dir/data/dbus-1/services"
sed "s|^Exec=.*|Exec=$binary --file-manager-service|" \
  "$project_root/data/org.freedesktop.FileManager1.service" \
  >"$smoke_dir/data/dbus-1/services/org.freedesktop.FileManager1.service"

XDG_DATA_HOME="$smoke_dir/data" dbus-run-session -- bash -euo pipefail -c '
  gdbus introspect \
    --session \
    --dest org.freedesktop.FileManager1 \
    --object-path /org/freedesktop/FileManager1 \
    | grep -F "ShowItems"
  if gdbus call \
    --session \
    --dest org.freedesktop.FileManager1 \
    --object-path /org/freedesktop/FileManager1 \
    --method org.freedesktop.FileManager1.ShowItems \
    "['"'"'file://remote/tmp/missing'"'"']" "" \
    >"$1/call.out" 2>&1
  then
    printf "%s\n" "ShowItems unexpectedly accepted a remote URI" >&2
    exit 1
  fi
  grep -F "org.freedesktop.DBus.Error.InvalidArgs" "$1/call.out"
' bash "$smoke_dir"

printf 'FileManager1 activation smoke test passed: %s\n' "$binary"
