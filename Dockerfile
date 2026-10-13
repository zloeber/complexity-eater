# Production image for the ce binary.
#
# Base images are pinned by manifest-list digest.
# Tags recorded on 2026-10-13:
#   rust:1.99-bookworm
#   debian:bookworm-slim
#   gcr.io/distroless/static-debian12:nonroot
#
# The runtime starts from distroless static and then copies the Debian 12
# libraries the binary loads: the dynamic linker, libc, libm, libgcc_s, and
# the NSS modules used for name resolution. It does not copy libssl. `ce`
# does not link OpenSSL, and the distroless cc image's libssl3 package had
# CVE-2026-84782 with no fixed version.
#
# Update with `docker buildx imagetools inspect <tag>` and keep the builder
# and the copied libraries on Debian 12 so the glibc they share stays
# compatible. Dependabot's docker updates are the review path for digest bumps.
# Do not move the runtime to Alpine or scratch without repeating the
# dependency check in docs/operations/container.md.

ARG RUST_IMAGE=rust:1.99-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6
ARG DEBIAN_IMAGE=debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
ARG STATIC_IMAGE=gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab

FROM ${DEBIAN_IMAGE} AS layout
RUN mkdir -p /var/lib/ce \
    && chown 65532:65532 /var/lib/ce \
    && chmod 0755 /var/lib/ce

FROM ${RUST_IMAGE} AS builder
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked --bin ce \
    && cp /src/target/release/ce /ce

FROM ${DEBIAN_IMAGE} AS runtime-libs
RUN set -eu \
    && apt-get update \
    && apt-get install -y --no-install-recommends libgcc-s1 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /out/etc/ssl/certs /out/var/lib/dpkg \
    && cp /etc/nsswitch.conf /out/etc/nsswitch.conf \
    && cp /etc/ssl/certs/ca-certificates.crt /out/etc/ssl/certs/ca-certificates.crt \
    && arch="$(dpkg --print-architecture)" \
    && case "$arch" in \
         amd64) triplet=x86_64-linux-gnu; ldso=/lib64/ld-linux-x86-64.so.2 ;; \
         arm64) triplet=aarch64-linux-gnu; ldso=/lib/ld-linux-aarch64.so.1 ;; \
         *) echo "unsupported architecture: $arch" >&2; exit 1 ;; \
       esac \
    && mkdir -p "/out/lib/${triplet}" \
    && for lib in libc.so.6 libm.so.6 libgcc_s.so.1 libnss_dns.so.2 libnss_files.so.2 libresolv.so.2; do \
         cp -a "/lib/${triplet}/${lib}" "/out/lib/${triplet}/${lib}"; \
       done \
    && if [ -L "$ldso" ]; then \
         target="$(readlink -f "$ldso")"; \
         mkdir -p "/out$(dirname "$target")" "/out$(dirname "$ldso")"; \
         cp -a "$target" "/out${target}"; \
         ln -s "$target" "/out${ldso}"; \
       else \
         mkdir -p "/out$(dirname "$ldso")"; \
         cp -a "$ldso" "/out${ldso}"; \
       fi \
    && dpkg-query -W -f 'Package: ${Package}\nStatus: install ok installed\nArchitecture: ${Architecture}\nVersion: ${Version}\n\n' \
         libc6 libgcc-s1 ca-certificates > /out/var/lib/dpkg/status \
    && if find /out \( -name 'libssl*' -o -name 'libcrypto*' -o -name 'ossl-modules' \) | grep -q .; then \
         echo "runtime root contains OpenSSL" >&2; \
         exit 1; \
       fi

FROM ${STATIC_IMAGE}
COPY --from=runtime-libs /out/ /
COPY --from=layout --chown=65532:65532 /var/lib/ce /var/lib/ce
COPY --from=builder --chown=65532:65532 --chmod=755 /ce /usr/local/bin/ce

ARG VERSION=0.1.0
ARG REVISION=unknown
ARG CREATED=unknown
ARG SOURCE=https://github.com/zloeber/complexity-eater
LABEL org.opencontainers.image.title="ce" \
      org.opencontainers.image.description="Complexity Eater room process" \
      org.opencontainers.image.source="${SOURCE}" \
      org.opencontainers.image.url="${SOURCE}" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"

USER 65532:65532
WORKDIR /var/lib/ce
ENV HOME=/var/lib/ce \
    TMPDIR=/tmp \
    CE_WORKSPACE=/var/lib/ce/workspace.md \
    XDG_CONFIG_HOME=/var/lib/ce/config \
    XDG_DATA_HOME=/var/lib/ce/data \
    XDG_STATE_HOME=/var/lib/ce/state
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/ce"]
CMD ["serve", "/var/lib/ce/workspace.md", "--name", "host"]
