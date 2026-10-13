---
title: Container
---

The production image is a multi-stage build. The builder is the pinned `rust:1.99-bookworm` image. The runtime starts from `gcr.io/distroless/static-debian12:nonroot` and copies the Debian 12 libraries the binary loads: the dynamic linker, `libc`, `libm`, `libgcc_s`, and the NSS modules for name resolution, plus the CA bundle and `nsswitch.conf`. Debian 12 is the pair because the binary is dynamically linked against that glibc. Alpine and `scratch` are the wrong runtime. The distroless `cc` image is also the wrong runtime: it ships `libssl3`, which `ce` does not link, and that package had an unfixed high-severity advisory.

`ce serve` does not make outbound TLS calls. The CA bundle is in the image so certificate files are present if a later build needs them. SQLite is inside the binary. The image has no shell, package manager, compiler, OpenSSL, or `tailcat`. The static base image includes zoneinfo. The container clock is the host clock.

## Identity and process

The process runs as UID and GID **65532**. The entrypoint is `["/usr/local/bin/ce"]`, so `ce` is the process that receives signals. Compose sets `init: true` so the runtime can reap child processes. `ce` handles SIGINT and SIGTERM and then shuts the room down.

The working directory is `/var/lib/ce`.

## Network

`ce` refuses every bind except loopback, and both listeners use `127.0.0.1`. A published container port does not reach that address. The Compose file sets `network_mode: host` so those listeners are the host's loopback. The process is still not privileged, it still drops every capability, and it still will not bind a public address.

Docker Desktop on macOS does not attach that namespace to the Mac's loopback. Run the binary directly on a Mac, or use a Linux host for Compose.

The owner page defaults to port 8000. The session ingress is a second loopback port chosen by the process and printed in its banner. Ports below 1024 are outside this profile because `NET_BIND_SERVICE` is dropped.

## Files

| Path | Use |
| --- | --- |
| `/var/lib/ce` | Workspace volume. The markdown file and `<file>.commonroom.db` are written here. |
| `/tmp` | `tmpfs`, `noexec`, `nosuid`, `nodev`. `--ephemeral` keeps the sidecar here and deletes it on exit. |

The root filesystem is read-only. A missing parent directory for the workspace fails with `invalid_request` and a filesystem error. The volume must be writable by UID 65532. A named volume picks up that ownership from the image. A bind mount must already be writable by that user.

Environment:

| Variable | Value in the image |
| --- | --- |
| `HOME` | `/var/lib/ce` |
| `TMPDIR` | `/tmp` |
| `CE_WORKSPACE` | `/var/lib/ce/workspace.md` |
| `XDG_CONFIG_HOME` | `/var/lib/ce/config` |
| `XDG_DATA_HOME` | `/var/lib/ce/data` |
| `XDG_STATE_HOME` | `/var/lib/ce/state` |

`RUST_LOG` is unset in the image. The process default is `warn`. Compose sets `RUST_LOG=warn` so the setting is visible. No credentials are passed as build arguments or image environment variables.

## Compose

`compose.yaml` is the hardened profile:

* non-root user `65532:65532`
* read-only root
* `cap_drop: ALL`
* `no-new-privileges`
* `privileged: false`
* host network, no published ports
* no Docker socket and no extra host mounts
* 1 CPU, 512 MB memory, 256 process ids
* 64 MB `tmpfs` on `/tmp`

The pid limit counts threads. 256 leaves room for the runtime's worker threads.

```sh
docker compose build
docker compose up
```

The human page is `http://127.0.0.1:8000/` on the Linux host. Stop the process with SIGTERM (`docker compose stop`). Do not mount `/var/run/docker.sock`.

## Check the image

```sh
task image-verify
```

That builds `ce:supply-chain`, checks the user, entrypoint, labels, and the absence of a shell, runs a read-only room under the limits above, checks SIGTERM, and scans the image with Trivy.
