#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.."
waddle_fault_dir=$(mktemp -d /tmp/waddle-audit-fault.XXXXXX)
trap 'rm -rf -- "$waddle_fault_dir"' EXIT
cc -shared -fPIC -Wall -Wextra -Werror .scratch/transfer-audit-14/open_fault.c -ldl -o "$waddle_fault_dir/open_fault.so"
WADDLE_AUDIT_SHIM="$waddle_fault_dir/open_fault.so" cargo test journal::tests::audit_ -- --ignored --nocapture --test-threads=1
