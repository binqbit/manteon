#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
source scripts/lib/rust.sh
if (( $# > 1 )); then
  echo 'Expected at most one argument; use --help.' >&2
  exit 2
fi
case "${1:-}" in
  "") ;;
  --restore)
    (
      core_toolchain="$(rust_toolchain .)"
      rustup toolchain install "$core_toolchain" --profile minimal --component rustfmt --component clippy
      use_rust_toolchain "$core_toolchain"
      CARGO_NET_OFFLINE=false "$CARGO" fetch --locked
    )
    ;;
  --help|-h)
    echo 'Usage: bash scripts/check.sh [--restore]'
    echo 'Check the pure core, Anchor SBF/IDL build, and local VM program tests.'
    echo '--restore installs pinned tools and fetches locked dependencies first.'
    exit 0 ;;
  *) echo "Unknown argument: $1" >&2; exit 2 ;;
esac
bash scripts/check-core.sh
bash scripts/check-anchor.sh "$@"
