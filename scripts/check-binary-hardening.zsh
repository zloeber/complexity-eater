#!/bin/zsh
# Check a release binary's platform hardening properties.
# Usage: scripts/check-binary-hardening.zsh path/to/ce
set -euo pipefail

if [[ $# -ne 1 ]]; then
  print -u2 "usage: scripts/check-binary-hardening.zsh <binary>"
  exit 2
fi

root="${0:A:h}/.."
exec python3 "$root/scripts/check_binary_hardening.py" "$1"
