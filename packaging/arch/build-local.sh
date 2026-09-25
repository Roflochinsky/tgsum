#!/usr/bin/env bash
# Builds the Arch/Omarchy package from this checkout: packs the working tree
# (committed and staged changes) as the release tarball the PKGBUILD expects,
# then runs makepkg. Extra arguments go to makepkg, e.g. `-si` to install
# dependencies and the built package.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)
pkgver=$(sed -n 's/^pkgver=//p' "$here/PKGBUILD")
tree=$(git -C "$root" stash create)

git -C "$root" archive --format=tar.gz --prefix="tgsum-$pkgver/" \
  -o "$here/tgsum-$pkgver.tar.gz" "${tree:-HEAD}"
cd "$here"
makepkg --cleanbuild --force "$@"
