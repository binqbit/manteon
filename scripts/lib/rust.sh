#!/usr/bin/env bash

rust_toolchain() {
  python3 -c 'import sys,tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["toolchain"]["channel"])' "$1/rust-toolchain.toml"
}

# Select subprocess compilers too, even when system Cargo precedes rustup in PATH.
use_rust_toolchain() {
  local version="$1"
  local selected_cargo
  local toolchain_bin
  selected_cargo="$(rustup which --toolchain "$version" cargo)"
  toolchain_bin="$(dirname "$selected_cargo")"
  export PATH="$toolchain_bin:$PATH"
  export RUSTUP_TOOLCHAIN="$version"
  export CARGO="$toolchain_bin/cargo"
  export RUSTC="$toolchain_bin/rustc"
  export RUSTDOC="$toolchain_bin/rustdoc"
}

cargo_for() (
  use_rust_toolchain "$1"
  export CARGO_TARGET_DIR="$2"
  shift 2
  "$CARGO" "$@"
)
