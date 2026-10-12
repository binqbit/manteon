#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
project_root="$PWD"
source scripts/lib/rust.sh

# Command options.
restore=false
for argument in "$@"; do
  case "$argument" in
    --restore) restore=true ;;
    --help|-h)
      echo 'Usage: bash scripts/check-anchor.sh [--restore]'
      echo 'Build Manteon, Tekmerion and the protocol IDL, then run local contract tests.'
      echo 'Default: offline. --restore installs exact tools and fetches locked dependencies.'
      exit 0 ;;
    *) echo "Unknown argument: $argument" >&2; exit 2 ;;
  esac
done

# Toolchains and build directories.
python3 -c 'import sys; assert sys.version_info >= (3, 11), "Python 3.11+ required"'
command -v rustup >/dev/null
program_dir="$project_root/programs/manteon"
runtime_dir="$project_root/tests/manteon"
auditor_dir="$project_root/vendor/tekmerion"
output_dir="$project_root/target"
if [[ ! -f "$project_root/vendor/tekmerion/sdk/rust/Cargo.toml" ]]; then
  echo 'Tekmerion submodule is missing; run git submodule update --init --recursive.' >&2
  exit 1
fi
export PATH="$output_dir/anchor-tools/bin:$PATH"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

program_toolchain="$(rust_toolchain "$program_dir")"
runtime_toolchain="$(rust_toolchain "$runtime_dir")"
# Anchor CLI's locked solana-sbpf dependency uses Layout::dangling_ptr (Rust 1.95).
# Tool compilation is independent of the program and LiteSVM compiler pins.
tools_toolchain='1.95.0'
# Keep unhashed libmanteon.rlib artifacts isolated between compiler versions.
program_host_dir="$output_dir/anchor-host/$program_toolchain"
runtime_host_dir="$output_dir/anchor-host/$runtime_toolchain"

has_pinned_anchor() {
  command -v anchor >/dev/null && [[ "$(anchor --version)" == 'anchor-cli 1.2.1' ]]
}

has_pinned_builder() {
  command -v cargo-build-sbf >/dev/null &&
    [[ "$(cargo-build-sbf --version | head -n 1)" == 'cargo-build-sbf 4.0.0' ]]
}

with_program_cargo() (
  rustup which --toolchain "$program_toolchain" cargo >/dev/null
  proxy_dir="$output_dir/anchor-cargo"
  mkdir -p "$proxy_dir"
  ln -sf "$project_root/scripts/anchor-cargo.sh" "$proxy_dir/cargo"
  export PATH="$proxy_dir:$PATH"
  export CARGO="$proxy_dir/cargo"
  export RUSTUP_TOOLCHAIN="$program_toolchain"
  cd "$program_dir"
  env -u RUSTC -u RUSTDOC "$@"
)

# Optional dependency restoration.
if "$restore"; then
  export CARGO_NET_OFFLINE=false
  for version in "$program_toolchain" "$runtime_toolchain"; do
    rustup toolchain install "$version" --profile minimal --component rustfmt --component clippy
  done
  if ! has_pinned_anchor || ! has_pinned_builder; then
    rustup toolchain install "$tools_toolchain" --profile minimal
  fi
  if ! has_pinned_anchor; then
    cargo_for "$tools_toolchain" "$output_dir/anchor-tool-build/$tools_toolchain" install \
      --locked --version 1.2.1 --root "$output_dir/anchor-tools" anchor-cli
  fi
  if ! has_pinned_builder; then
    cargo_for "$tools_toolchain" "$output_dir/anchor-tool-build/$tools_toolchain" install \
      --locked --version 4.0.0 --root "$output_dir/anchor-tools" cargo-build-sbf
  fi
  for manifest_dir in "$program_dir" "$runtime_dir"; do
    cargo_for "$(rust_toolchain "$manifest_dir")" "$output_dir/anchor-fetch" fetch \
      --locked --manifest-path "$manifest_dir/Cargo.toml"
  done
  cargo_for "$program_toolchain" "$output_dir/anchor-fetch" fetch \
    --locked --manifest-path "$auditor_dir/Cargo.toml"
  with_program_cargo cargo-build-sbf --install-only --tools-version v1.53
fi

# Formatting and static checks.
if ! has_pinned_anchor || ! has_pinned_builder; then
  echo 'Anchor CLI 1.2.1 and cargo-build-sbf 4.0.0 required; run with --restore.' >&2
  exit 1
fi
export CARGO_NET_OFFLINE=true
for manifest_dir in "$program_dir" "$runtime_dir"; do
  version="$(rust_toolchain "$manifest_dir")"
  "$(rustup which --toolchain "$version" rustc)" --version
  host_target="$output_dir/anchor-host/$version"
  cargo_for "$version" "$host_target" fmt \
    --manifest-path "$manifest_dir/Cargo.toml" --all -- --check
  cargo_for "$version" "$host_target" clippy \
    --manifest-path "$manifest_dir/Cargo.toml" --workspace --all-targets --locked --offline -- -D warnings
done

# Program builds.
# Keep the auditor's deployable SBF build separate from its CPI library.
for program in manteon tekmerion; do
  manifest="$program_dir/Cargo.toml"
  sbf_target="$output_dir/anchor-sbf"
  if [[ "$program" == tekmerion ]]; then
    manifest="$auditor_dir/programs/tekmerion/Cargo.toml"
    sbf_target="$output_dir/tekmerion-sbf"
  fi
  build_log="$output_dir/$program-sbf-build.log"
  CARGO_TERM_COLOR=never CARGO_TARGET_DIR="$sbf_target" \
    with_program_cargo cargo-build-sbf --manifest-path "$manifest" \
    --sbf-out-dir "$output_dir/deploy" --tools-version v1.53 --arch v0 \
    --offline --skip-tools-install --jobs "$CARGO_BUILD_JOBS" -- --package "$program" --locked \
    2>&1 | tee "$build_log"
  bash "$project_root/scripts/check-sbf-log.sh" "$build_log"
done

# Generated IDL and published protocol compatibility.
mkdir -p "$output_dir/idl" "$output_dir/types"
(
  use_rust_toolchain "$program_toolchain"
  export CARGO_TARGET_DIR="$output_dir/anchor-idl"
  cd "$program_dir"
  # Anchor 1.2.1 interpolates RUSTUP_TOOLCHAIN incorrectly in IDL builds.
  # Select the actual binaries explicitly instead of its +toolchain route.
  env -u RUSTUP_TOOLCHAIN anchor idl build -p manteon \
    --out "$output_dir/idl/manteon.json" --out-ts "$output_dir/types/manteon.ts" \
    -- --locked --offline
)

# Compare generated and published IDLs structurally, independent of formatting.
python3 - "$output_dir/idl/manteon.json" "$project_root/idl/manteon.json" <<'PY'
import json
import sys
from pathlib import Path

actual, published = (json.loads(Path(path).read_text()) for path in sys.argv[1:])
if actual != published:
    raise SystemExit("Published IDL differs from Anchor output; update idl/manteon.json from target/idl/manteon.json")
PY

# Host and VM tests.
export MANTEON_PROGRAM_SO="$output_dir/deploy/manteon.so"
export TEKMERION_PROGRAM_SO="$output_dir/deploy/tekmerion.so"
export MANTEON_IDL_JSON="$output_dir/idl/manteon.json"
cargo_for "$program_toolchain" "$program_host_dir" test \
  --manifest-path "$program_dir/Cargo.toml" --workspace --locked --offline
cargo_for "$runtime_toolchain" "$runtime_host_dir" test \
  --manifest-path "$runtime_dir/Cargo.toml" --locked --offline -- --test-threads=2
echo 'Manteon contract checks passed, including the pinned Tekmerion integration.'
