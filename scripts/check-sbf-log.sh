#!/usr/bin/env bash
set -euo pipefail

build_log="${1:?Usage: bash scripts/check-sbf-log.sh BUILD_LOG}"
[[ -r "$build_log" ]] || { echo "Cannot read SBF build log: $build_log" >&2; exit 1; }

# The SBF compiler can report an unusable artifact while returning success.
if LC_ALL=C grep -E 'Error:|Stack offset|Unknown syscall|undefined and not known syscalls|Calling them will trigger a run-time error' "$build_log" >&2; then
  echo 'SBF build rejected: compiler diagnostics indicate a runtime failure.' >&2
  exit 1
fi
