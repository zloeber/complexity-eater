#!/bin/zsh
# Assemble the public website into website/dist.
set -euo pipefail

root="${0:A:h}/.."
python3 "$root/scripts/website.py" build
