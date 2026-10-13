#!/bin/zsh
# Fail on high or critical RustSec advisories. Print lower findings.
# A missing database or a cargo-audit failure is a failure, not a pass.
# Usage: scripts/cargo-audit-gate.zsh
set -euo pipefail

root="${0:A:h}/.."
cd "$root"

if ! command -v cargo-audit >/dev/null 2>&1; then
  print -u2 "cargo-audit is not installed. Run scripts/install-supply-chain-tools.zsh"
  exit 1
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

set +e
cargo audit --json >"$tmpdir/audit.json" 2>"$tmpdir/audit.err"
audit_status=$?
set -e

python3 - "$tmpdir/audit.json" "$tmpdir/audit.err" "$audit_status" <<'PY'
import json
import re
import sys
from pathlib import Path

report_path, err_path, status = sys.argv[1:]
status = int(status)
raw = Path(report_path).read_text()
err = Path(err_path).read_text()
if not raw.strip():
    sys.stderr.write(err or "cargo audit produced no report\n")
    raise SystemExit(1)
try:
    report = json.loads(raw)
except json.JSONDecodeError:
    sys.stderr.write(err or raw)
    raise SystemExit(1)

def cvss_score(advisory):
    cvss = advisory.get("cvss")
    if isinstance(cvss, dict):
        for key in ("score", "base_score", "baseScore"):
            if isinstance(cvss.get(key), (int, float)):
                return float(cvss[key])
        vector = cvss.get("vector") or cvss.get("vector_string")
    else:
        vector = cvss
    severity = advisory.get("severity")
    named = {
        "none": 0.0,
        "low": 3.9,
        "medium": 6.9,
        "high": 8.9,
        "critical": 10.0,
    }
    if isinstance(severity, str) and severity.lower() in named:
        return named[severity.lower()]
    if isinstance(vector, str):
        # RustSec sometimes stores the numeric score in the cvss string.
        match = re.search(r"\b(\d+(?:\.\d+)?)\b", vector)
        if "/" not in vector and match:
            return float(match.group(1))
    return None

vulnerabilities = report.get("vulnerabilities") or {}
items = vulnerabilities.get("list") or []
blocking = []
reported = []
for item in items:
    advisory = item.get("advisory") or {}
    ident = advisory.get("id") or "unknown"
    package = advisory.get("package") or item.get("package") or {}
    if isinstance(package, dict):
        name = package.get("name") or "unknown"
    else:
        name = str(package)
    title = advisory.get("title") or ""
    score = cvss_score(advisory)
    line = f"{ident} {name} score={score if score is not None else 'unrated'} {title}".strip()
    # Unrated vulnerabilities fail closed. High is 7.0 and above.
    if score is None or score >= 7.0:
        blocking.append(line)
    else:
        reported.append(line)

warnings = report.get("warnings") or {}
for kind, entries in warnings.items():
    if not isinstance(entries, list):
        continue
    for entry in entries:
        if isinstance(entry, dict):
            advisory = entry.get("advisory") or entry
            ident = advisory.get("id") or kind
            name = advisory.get("package") or entry.get("package") or ""
            reported.append(f"warning {kind} {ident} {name}".strip())
        else:
            reported.append(f"warning {kind} {entry}")

if reported:
    print("Lower-severity advisory findings:")
    for line in reported:
        print(f"  {line}")
else:
    print("No medium, low, or warning advisories.")

if blocking:
    print("Blocking advisory findings:", file=sys.stderr)
    for line in blocking:
        print(f"  {line}", file=sys.stderr)
    raise SystemExit(1)

if status not in (0, 1):
    sys.stderr.write(err or f"cargo audit exited {status}\n")
    raise SystemExit(status)

# cargo audit exits 1 when any vulnerability exists. Lower-severity findings
# are reported above and do not fail this gate. cargo-deny still fails the
# build on every advisory vulnerability.
if status == 1 and not items and not reported:
    sys.stderr.write(err or "cargo audit failed without a parsed finding\n")
    raise SystemExit(1)

print("RustSec high and critical gate passed.")
PY
