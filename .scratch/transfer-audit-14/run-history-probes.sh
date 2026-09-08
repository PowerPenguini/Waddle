#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.."
cargo test journal::tests::audit_ -- --include-ignored --nocapture --test-threads=1
