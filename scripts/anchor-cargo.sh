#!/usr/bin/env bash
set -euo pipefail

# Anchor IDL generation and cargo-build-sbf invoke `cargo +<toolchain>`.
# Route that syntax explicitly on hosts whose Cargo is not a rustup proxy.
# The caller pins the host compiler; build-sbf selects its platform compiler.
selected_toolchain="${RUSTUP_TOOLCHAIN:?Anchor runner must select a host toolchain}"
if [[ "${1:-}" == +* ]]; then
  selected_toolchain="${1:1}"
  shift
fi
selected_cargo="$(rustup which --toolchain "$selected_toolchain" cargo)"
selected_bin="$(dirname "$selected_cargo")"
export PATH="$selected_bin:$PATH"
export RUSTC="$selected_bin/rustc"
export RUSTDOC="$selected_bin/rustdoc"
exec "$selected_cargo" "$@"
