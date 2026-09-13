#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
action=install
if [ "${1:-}" = --uninstall ]; then action=uninstall; shift; fi
if [ -x "$project_dir/bin/antigravity-region-fix" ]; then
    exec "$project_dir/bin/antigravity-region-fix" "$action" "$@"
fi
command -v cargo >/dev/null 2>&1 || { echo 'Rust 1.88+ is required to build from source; native release bundles need no Rust or Python.' >&2; exit 1; }
cargo build --manifest-path "$project_dir/Cargo.toml" --target-dir "$project_dir/target" --release --locked --bins
exec "$project_dir/target/release/antigravity-region-fix" "$action" "$@"
