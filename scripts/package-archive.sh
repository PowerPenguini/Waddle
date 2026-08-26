#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$project_root/Cargo.toml" | head -n 1)
architecture=$(uname -m)
archive_name="waddle-${version}-${architecture}-linux"
distribution_dir="$project_root/dist"
stage_dir=$(mktemp -d "/tmp/${archive_name}.XXXXXX")
trap 'rm -rf -- "$stage_dir"' EXIT

cargo build --manifest-path "$project_root/Cargo.toml" --release --locked
install -Dm0755 "$project_root/target/release/waddle" "$stage_dir/$archive_name/bin/waddle"
install -Dm0644 "$project_root/README.md" "$stage_dir/$archive_name/README.md"
install -Dm0644 "$project_root/data/io.github.powerpenguini.Waddle.desktop" \
  "$stage_dir/$archive_name/share/applications/io.github.powerpenguini.Waddle.desktop"
install -Dm0644 "$project_root/data/io.github.powerpenguini.Waddle.metainfo.xml" \
  "$stage_dir/$archive_name/share/metainfo/io.github.powerpenguini.Waddle.metainfo.xml"
install -Dm0644 "$project_root/data/icons/hicolor/scalable/apps/io.github.powerpenguini.Waddle.svg" \
  "$stage_dir/$archive_name/share/icons/hicolor/scalable/apps/io.github.powerpenguini.Waddle.svg"

mkdir -p "$distribution_dir"
tar --create --gzip --file "$distribution_dir/$archive_name.tar.gz" \
  --directory "$stage_dir" "$archive_name"
printf '%s\n' "$distribution_dir/$archive_name.tar.gz"
