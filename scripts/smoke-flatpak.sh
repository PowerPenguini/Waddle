#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  printf 'usage: %s BUNDLE\n' "$0" >&2
  exit 2
fi

bundle=$(realpath -- "$1")
app_id=io.github.powerpenguini.Waddle
flatpak install --user --noninteractive --or-update "$bundle"
cleanup() {
  flatpak uninstall --user --noninteractive "$app_id" >/dev/null 2>&1 || true
}
trap cleanup EXIT
flatpak run --user --command=sh "$app_id" -c \
  'test -x /app/bin/waddle && test -f /app/share/applications/io.github.powerpenguini.Waddle.desktop'
printf 'Flatpak smoke test passed: %s\n' "$bundle"
