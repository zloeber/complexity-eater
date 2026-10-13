#!/bin/zsh
# Install pinned supply-chain binaries into target/supply-chain-bin.
# Checksums are the SHA-256 of the upstream release archives.
set -euo pipefail

root="${0:A:h}/.."
dest="$root/target/supply-chain-bin"
mkdir -p "$dest"
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
  Darwin:arm64) platform=darwin-arm64 ;;
  Darwin:x86_64) platform=darwin-x64 ;;
  Linux:x86_64) platform=linux-x64 ;;
  *)
    print -u2 "unsupported platform: $os $arch"
    exit 1
    ;;
esac

typeset -A URL SHA
case "$platform" in
  darwin-arm64)
    URL[gitleaks]=https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_darwin_arm64.tar.gz
    SHA[gitleaks]=b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5
    URL[cargo-deny]=https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-aarch64-apple-darwin.tar.gz
    SHA[cargo-deny]=fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3
    URL[cargo-audit]=https://github.com/rustsec/rustsec/releases/download/cargo-audit/v0.22.2/cargo-audit-aarch64-apple-darwin-v0.22.2.tgz
    SHA[cargo-audit]=ec7ca4263769593df4d909be85b94a6b79efa2897be5d2bb8ebd516e823175af
    URL[cargo-cyclonedx]=https://github.com/CycloneDX/cyclonedx-rust-cargo/releases/download/cargo-cyclonedx-0.5.9/cargo-cyclonedx-aarch64-apple-darwin.tar.xz
    SHA[cargo-cyclonedx]=4c53dfa21e70b65bf7f8d2592aadde3bcb02c1a40b6ec63b877e5ca65a29e180
    URL[trivy]=https://github.com/aquasecurity/trivy/releases/download/v0.75.0/trivy_0.75.0_macOS-ARM64.tar.gz
    SHA[trivy]=4a77108cccf8e55c8d6823e1e759939a622277e66cd0daa3c1fc621ed69e4568
    ;;
  darwin-x64)
    URL[gitleaks]=https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_darwin_x64.tar.gz
    SHA[gitleaks]=dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709
    URL[cargo-deny]=https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-apple-darwin.tar.gz
    SHA[cargo-deny]=248da7f581724e470071990c088ffc55c811981715f4cbdb258621fb79f8b7a6
    URL[cargo-audit]=https://github.com/rustsec/rustsec/releases/download/cargo-audit/v0.22.2/cargo-audit-x86_64-apple-darwin-v0.22.2.tgz
    SHA[cargo-audit]=847831323de932155b226ab60ee4a180e13e5d007a019f0d4b7b4d89a6de2ab2
    URL[cargo-cyclonedx]=https://github.com/CycloneDX/cyclonedx-rust-cargo/releases/download/cargo-cyclonedx-0.5.9/cargo-cyclonedx-x86_64-apple-darwin.tar.xz
    SHA[cargo-cyclonedx]=59d2a583fa632f8759456c1b531340331255b277386d23c598a3dbbc916fde63
    URL[trivy]=https://github.com/aquasecurity/trivy/releases/download/v0.75.0/trivy_0.75.0_macOS-64bit.tar.gz
    SHA[trivy]=291edaa9778acbe4693d067b5ad60ee11570e5ac68296e85595417528ca641e4
    ;;
  linux-x64)
    URL[gitleaks]=https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz
    SHA[gitleaks]=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
    URL[cargo-deny]=https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz
    SHA[cargo-deny]=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
    URL[cargo-audit]=https://github.com/rustsec/rustsec/releases/download/cargo-audit/v0.22.2/cargo-audit-x86_64-unknown-linux-gnu-v0.22.2.tgz
    SHA[cargo-audit]=ab28a1bdb54db4d5d8ad5981cf1f959410370b3d28250dbd35f6a44248620e39
    URL[cargo-cyclonedx]=https://github.com/CycloneDX/cyclonedx-rust-cargo/releases/download/cargo-cyclonedx-0.5.9/cargo-cyclonedx-x86_64-unknown-linux-gnu.tar.xz
    SHA[cargo-cyclonedx]=fb8dbee9f182173e062a64a387b21a0badc6fab8b2abf9294973f012972bf6d8
    URL[trivy]=https://github.com/aquasecurity/trivy/releases/download/v0.75.0/trivy_0.75.0_Linux-64bit.tar.gz
    SHA[trivy]=c6e65abddb348e25f10549df887045629cf28cc72453cd1c63acb717316b3f3f
    ;;
esac

install_tool() {
  local name="$1"
  local url="${URL[$name]}"
  local expect="${SHA[$name]}"
  local archive="$tmpdir/${name}.archive"
  local unpack="$tmpdir/${name}"
  print "installing ${name}"
  curl -fsSL "$url" -o "$archive"
  local actual
  actual=$(shasum -a 256 "$archive" | awk '{print $1}')
  if [[ "$actual" != "$expect" ]]; then
    print -u2 "${name} checksum mismatch"
    print -u2 "expected ${expect}"
    print -u2 "actual   ${actual}"
    exit 1
  fi
  mkdir -p "$unpack"
  tar -xf "$archive" -C "$unpack"
  local found
  found=$(find "$unpack" -type f -name "$name" -print -quit)
  if [[ -z "$found" ]]; then
    print -u2 "did not find ${name} inside ${url}"
    exit 1
  fi
  install -m 0755 "$found" "$dest/$name"
}

stamp="$dest/.stamp"
expected="${SHA[gitleaks]} ${SHA[cargo-deny]} ${SHA[cargo-audit]} ${SHA[cargo-cyclonedx]} ${SHA[trivy]}"
if [[ -f "$stamp" && "$(<"$stamp")" == "$expected" && -x "$dest/trivy" && -x "$dest/gitleaks" ]]; then
  print "supply-chain tools already installed in ${dest}"
  print "export PATH=\"${dest}:\$PATH\""
  exit 0
fi

install_tool gitleaks
install_tool cargo-deny
install_tool cargo-audit
install_tool cargo-cyclonedx
install_tool trivy
print -r -- "$expected" > "$stamp"

print "installed supply-chain tools in ${dest}"
print "export PATH=\"${dest}:\$PATH\""
