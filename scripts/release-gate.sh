#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_root"

cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release --locked

if [[ -n "${DISPLAY:-}" ]]; then
  WADDLE_X11_TEST=1 cargo test x11 -- --test-threads=1
else
  printf '%s\n' 'Skipping real-X11 adapter tests because DISPLAY is unset.'
fi

desktop-file-validate data/io.github.powerpenguini.Waddle.desktop
appstreamcli validate --pedantic --no-net data/io.github.powerpenguini.Waddle.metainfo.xml
archive=$(scripts/package-archive.sh)
scripts/smoke-package.sh "$archive"
git diff --check

printf '%s\n' 'Automated Waddle release gate passed.'
