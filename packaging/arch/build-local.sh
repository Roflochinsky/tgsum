#!/usr/bin/env bash
# Builds the Arch/Omarchy package from this checkout: packs the working tree
# (committed and staged changes) as the release tarball the PKGBUILD expects,
# then runs makepkg. Extra arguments go to makepkg, e.g. `-si` to install
# dependencies and the built package.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)
pkgver=$(sed -n 's/^pkgver=//p' "$here/PKGBUILD")

# `git stash create` exits 1 without a word when files differ from the index
# only in stat data (a fresh CI checkout, a chown), so let `git status`
# refresh the index and decide whether there is anything to pack first.
if [ -n "$(git -C "$root" status --porcelain --untracked-files=no)" ]; then
  tree=$(git -C "$root" stash create)
else
  tree=HEAD
fi

git -C "$root" archive --format=tar.gz --prefix="tgsum-$pkgver/" \
  -o "$here/tgsum-$pkgver.tar.gz" "$tree"
cd "$here"
makepkg --cleanbuild --force "$@"
