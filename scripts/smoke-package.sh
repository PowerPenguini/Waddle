#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  printf 'usage: %s ARCHIVE\n' "$0" >&2
  exit 2
fi

archive=$(realpath -- "$1")
smoke_dir=$(mktemp -d /tmp/waddle-package-smoke.XXXXXX)
trap 'rm -rf -- "$smoke_dir"' EXIT
tar --extract --gzip --file "$archive" --directory "$smoke_dir"
package_root=$(find "$smoke_dir" -mindepth 1 -maxdepth 1 -type d -name 'waddle-*-linux' -print -quit)
test -n "$package_root"
test -x "$package_root/bin/waddle"
desktop-file-validate "$package_root/share/applications/io.github.powerpenguini.Waddle.desktop"
service_file="$package_root/share/dbus-1/services/org.freedesktop.FileManager1.service"
test -f "$service_file"
grep -Fx 'Name=org.freedesktop.FileManager1' "$service_file"
grep -Fx 'Exec=/usr/bin/waddle --file-manager-service' "$service_file"
appstreamcli validate --no-net "$package_root/share/metainfo/io.github.powerpenguini.Waddle.metainfo.xml"
rsvg-convert "$package_root/share/icons/hicolor/scalable/apps/io.github.powerpenguini.Waddle.svg" \
  --width 128 --height 128 --output "$smoke_dir/icon.png"
test -s "$smoke_dir/icon.png"
if ldd "$package_root/bin/waddle" | grep -F 'not found'; then
  exit 1
fi
if [[ -n "${WAYLAND_DISPLAY:-}" || -n "${DISPLAY:-}" ]]; then
  mkdir -p "$smoke_dir/state" "$smoke_dir/cache" "$smoke_dir/config" "$smoke_dir/data"
  set +e
  XDG_STATE_HOME="$smoke_dir/state" \
    XDG_CACHE_HOME="$smoke_dir/cache" \
    XDG_CONFIG_HOME="$smoke_dir/config" \
    XDG_DATA_HOME="$smoke_dir/data" \
    timeout 3 "$package_root/bin/waddle" "$smoke_dir" >/dev/null 2>&1
  launch_status=$?
  set -e
  if [[ $launch_status -ne 0 && $launch_status -ne 124 ]]; then
    printf 'Waddle launch failed with status %s\n' "$launch_status" >&2
    exit "$launch_status"
  fi
fi
printf 'Archive smoke test passed: %s\n' "$archive"
