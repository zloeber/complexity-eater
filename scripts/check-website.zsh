#!/bin/zsh
# Build the public website and verify links, the launch contract, and release assets.
set -euo pipefail

root="${0:A:h}/.."
python3 "$root/scripts/website.py" check "$@"
