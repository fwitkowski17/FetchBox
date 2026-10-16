#!/usr/bin/env bash
# Builds the DEB and the RPM of one architecture from a release binary:
#
#   packaging/build.sh <version> <amd64|arm64> <path of the binary> <output directory>
#
# Needs nfpm. Writes fetchbox_<version>_<arch>.deb and fetchbox-<version>-1.<x86_64|aarch64>.rpm, each
# with a .sha256 file, into the output directory. Nothing of the binary is executed, so the packages
# of another CPU can be made on any machine.
set -euo pipefail

[[ $# -eq 4 ]] || {
  sed -n '2,8p' "$0" >&2
  exit 2
}
version="$1" arch="$2" binary="$(realpath "$3")" out="$4"
case "$arch" in
  amd64) rpm_arch="x86_64" ;;
  arm64) rpm_arch="aarch64" ;;
  *)
    echo "error: the architecture must be amd64 or arm64" >&2
    exit 2
    ;;
esac
[[ -f "$binary" ]] || {
  echo "error: no binary at $binary" >&2
  exit 1
}

cd "$(dirname "$0")/.."
mkdir -p "$out"
out="$(realpath "$out")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

for format in deb rpm; do
  VERSION="$version" ARCH="$arch" BINARY="$binary" nfpm package -f packaging/nfpm.yaml -p "$format" -t "$work"
done

# nfpm writes a pre-release as 0.7.0~rc.1, a character that GitHub rewrites in file names: the
# names here keep the tag's own spelling (0.7.0-rc.1).
deb="fetchbox_${version}_${arch}.deb"
rpm="fetchbox-${version}-1.${rpm_arch}.rpm"
mv "$work"/*.deb "${out}/${deb}"
mv "$work"/*.rpm "${out}/${rpm}"
(cd "$out" && sha256sum "$deb" >"${deb}.sha256" && sha256sum "$rpm" >"${rpm}.sha256")
echo "built ${out}/${deb} and ${out}/${rpm}"
