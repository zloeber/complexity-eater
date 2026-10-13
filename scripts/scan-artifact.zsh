#!/bin/zsh
# Scan a file or container image.
# High and critical vulnerabilities fail the command. Medium, low, and
# unknown findings are printed and do not fail. Secret findings fail.
# A scanner or database error fails the command.
# Usage: scripts/scan-artifact.zsh file|image <target>
set -euo pipefail

if [[ $# -ne 2 ]]; then
  print -u2 "usage: scripts/scan-artifact.zsh file|image <target>"
  exit 2
fi

kind="$1"
target="$2"
root="${0:A:h}/.."
export TRIVY_CACHE_DIR="${TRIVY_CACHE_DIR:-$root/target/trivy-cache}"
mkdir -p "$TRIVY_CACHE_DIR"

if ! command -v trivy >/dev/null 2>&1; then
  print -u2 "trivy is not installed. Run scripts/install-supply-chain-tools.zsh"
  exit 1
fi

case "$kind" in
  file) base=(trivy fs) ;;
  image) base=(trivy image) ;;
  *)
    print -u2 "usage: scripts/scan-artifact.zsh file|image <target>"
    exit 2
    ;;
esac

print "secret scan: ${kind} ${target}"
"${base[@]}" --scanners secret --exit-code 1 --no-progress "$target"

print "vulnerability scan (critical, high): ${kind} ${target}"
"${base[@]}" --scanners vuln --severity HIGH,CRITICAL --exit-code 1 --ignore-unfixed=false --no-progress "$target"

print "vulnerability report (unknown, low, medium): ${kind} ${target}"
"${base[@]}" --scanners vuln --severity UNKNOWN,LOW,MEDIUM --exit-code 0 --ignore-unfixed=false --no-progress "$target"

if [[ "$kind" == "image" ]]; then
  print "image configuration scan (critical, high)"
  "${base[@]}" --scanners misconfig --severity HIGH,CRITICAL --exit-code 1 --no-progress "$target"
fi

print "scan gate passed for ${target}"
