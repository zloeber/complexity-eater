#!/bin/zsh
# Build the production image and exercise the hardened runtime profile.
# Usage: scripts/verify-container.zsh
set -euo pipefail

root="${0:A:h}/.."
cd "$root"
image="${CE_IMAGE:-ce:supply-chain}"
port="${CE_VERIFY_PORT:-18765}"
name="ce-supply-chain-$$"
data=$(mktemp -d)
files=$(mktemp)

cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
  rm -rf "$data" "$files"
}
trap cleanup EXIT

if ! command -v docker >/dev/null 2>&1; then
  print -u2 "docker is not available"
  exit 1
fi

zsh "$root/scripts/build-image.zsh"

debian=$(
  python3 - <<'PY'
import re
from pathlib import Path
text = Path("Dockerfile").read_text()
match = re.search(r"DEBIAN_IMAGE=(debian:bookworm-slim@sha256:[0-9a-f]+)", text)
if not match:
    raise SystemExit("Dockerfile is missing the pinned Debian image")
print(match.group(1))
PY
)

print "checking image identity"
python3 - "$image" <<'PY'
import json
import subprocess
import sys

image = sys.argv[1]
config = json.loads(subprocess.check_output(["docker", "image", "inspect", image]))[0]["Config"]
user = config.get("User") or ""
if user != "65532:65532":
    raise SystemExit(f"image user is {user!r}")
entrypoint = config.get("Entrypoint") or []
if entrypoint != ["/usr/local/bin/ce"]:
    raise SystemExit(f"entrypoint is {entrypoint!r}")
if config.get("Cmd") != ["serve", "/var/lib/ce/workspace.md", "--name", "host"]:
    raise SystemExit(f"cmd is {config.get('Cmd')!r}")
labels = config.get("Labels") or {}
for key in (
    "org.opencontainers.image.source",
    "org.opencontainers.image.version",
    "org.opencontainers.image.revision",
    "org.opencontainers.image.licenses",
):
    if not labels.get(key):
        raise SystemExit(f"missing label {key}")
banned = ("TOKEN", "PASSWORD", "SECRET", "CREDENTIAL", "API_KEY", "PRIVATE_KEY")
for item in config.get("Env") or []:
    name, _, value = item.partition("=")
    if any(word in name.upper() for word in banned):
        raise SystemExit(f"unexpected environment variable {name}")
    if value.startswith(("ghp_", "github_pat_", "AKIA", "xox")):
        raise SystemExit(f"environment value for {name} looks like a credential")
print("image identity checks passed")
PY

print "checking image contents"
created=$(docker create "$image")
docker export "$created" | tar -tf - >"$files"
docker rm "$created" >/dev/null
python3 - "$files" <<'PY'
import sys
from pathlib import Path

names = {line.strip().lstrip("./") for line in Path(sys.argv[1]).read_text().splitlines()}
required = [
    "usr/local/bin/ce",
    "etc/ssl/certs/ca-certificates.crt",
    "etc/nsswitch.conf",
]
missing = [item for item in required if item not in names and item + "/" not in names]
if missing:
    raise SystemExit("image is missing " + ", ".join(missing))
banned = [
    "bin/sh",
    "bin/bash",
    "usr/bin/sh",
    "usr/bin/bash",
    "usr/bin/apt",
    "usr/bin/apt-get",
    "usr/bin/curl",
    "usr/bin/wget",
    "usr/bin/cargo",
    "usr/bin/rustc",
    "bin/busybox",
    "usr/bin/busybox",
]
present = [item for item in banned if item in names]
if present:
    raise SystemExit("image contains " + ", ".join(present))
openssl = sorted(name for name in names if "libssl" in name or "libcrypto" in name or "ossl-modules" in name)
if openssl:
    raise SystemExit("image contains OpenSSL: " + ", ".join(openssl))
if any(name.startswith(".git/") or name == ".git" for name in names):
    raise SystemExit("image contains git metadata")
print("image content checks passed")
PY

print "checking the image binary"
elf=$(mktemp)
elf_container=$(docker create "$image")
docker cp "$elf_container:/usr/local/bin/ce" "$elf" >/dev/null
docker rm "$elf_container" >/dev/null
python3 "$root/scripts/check_binary_hardening.py" "$elf"
rm -f "$elf"

print "checking ce --version"
docker run --rm \
  --read-only \
  --user 65532:65532 \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
  "$image" --version

print "checking a missing writable workspace"
set +e
missing_output=$(
  docker run --rm \
    --read-only \
    --user 65532:65532 \
    --cap-drop=ALL \
    --security-opt no-new-privileges \
    "$image" serve /usr/share/ce-missing/workspace.md --name host --port 18766 2>&1
)
missing_status=$?
set -e
print "$missing_output"
if [[ "$missing_status" -eq 0 || -z "$missing_output" ]]; then
  print -u2 "expected a non-empty failure for a read-only workspace path"
  exit 1
fi

chmod 0777 "$data"
run_room() {
  local mode="$1"
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker run -d \
    --name "$name" \
    --init \
    --read-only \
    --user 65532:65532 \
    --cap-drop=ALL \
    --security-opt no-new-privileges \
    --pids-limit 256 \
    --memory 512m \
    --tmpfs /tmp:rw,noexec,nosuid,nodev,size=64m \
    --volume "$data:/var/lib/ce" \
    --network host \
    "$image" \
    serve /var/lib/ce/workspace.md --name host --port "$port" ${mode:+"--$mode"}
  local ready=0
  local attempt
  local logs
  for attempt in {1..30}; do
    logs=$(docker logs "$name" 2>&1 || true)
    if [[ "$logs" == *"Complexity Eater"* ]]; then
      ready=1
      break
    fi
    sleep 1
  done
  if [[ "$ready" -ne 1 ]]; then
    print -u2 "$logs"
    print -u2 "ce did not become ready"
    exit 1
  fi
  local page
  page=$(
    docker run --rm --network "container:${name}" "$debian" \
      bash -c "exec 3<>/dev/tcp/127.0.0.1/${port}; printf 'GET / HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n' >&3; cat <&3" \
      || true
  )
  if [[ "$page" != *"Complexity Eater"* ]]; then
    print -u2 "owner page probe failed"
    print -u2 "$page"
    exit 1
  fi
  docker kill --signal=TERM "$name" >/dev/null
  local code
  code=$(docker wait "$name")
  if [[ "$code" != "0" ]]; then
    docker logs "$name" >&2 || true
    print -u2 "SIGTERM exit code was ${code}"
    exit 1
  fi
}

print "checking ephemeral startup and SIGTERM"
run_room ephemeral
if [[ ! -f "$data/workspace.md" ]]; then
  print -u2 "ephemeral mode did not leave the markdown file on the volume"
  exit 1
fi
if [[ -e "$data/workspace.md.commonroom.db" ]]; then
  print -u2 "ephemeral mode left the sidecar on the volume"
  exit 1
fi

# Keep the markdown file from the ephemeral run. Deleting it on the host and
# recreating it in the next container races Docker Desktop's file share.
print "checking persistent sidecar data"
run_room ""
if [[ ! -f "$data/workspace.md" || ! -f "$data/workspace.md.commonroom.db" ]]; then
  print -u2 "persistent mode did not write the markdown file and sidecar"
  ls -la "$data" >&2 || true
  exit 1
fi

print "checking shell absence"
set +e
docker run --rm --entrypoint /bin/sh "$image" -c true >/dev/null 2>&1
shell_status=$?
set -e
if [[ "$shell_status" -eq 0 ]]; then
  print -u2 "image started /bin/sh"
  exit 1
fi

print "checking compose profile"
rendered=$(docker compose -f "$root/compose.yaml" config)
python3 - "$rendered" <<'PY'
import sys
from pathlib import Path

rendered = sys.argv[1]
source = Path("compose.yaml").read_text()
if "privileged: false" not in source:
    raise SystemExit("compose.yaml does not set privileged: false")
if "privileged: true" in rendered:
    raise SystemExit("compose config enables privileged mode")
required = [
    "read_only: true",
    "network_mode: host",
    "no-new-privileges:true",
    "ALL",
    "65532:65532",
]
missing = [item for item in required if item not in rendered]
if missing:
    raise SystemExit("compose config is missing " + ", ".join(missing))
if "docker.sock" in rendered or "docker.sock" in source:
    raise SystemExit("compose config mounts the Docker socket")
print("compose profile checks passed")
PY

if command -v trivy >/dev/null 2>&1; then
  zsh "$root/scripts/scan-artifact.zsh" image "$image"
else
  print -u2 "trivy is not installed; image scan was not run"
  exit 1
fi

print "container verification passed"
