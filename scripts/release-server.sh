#!/usr/bin/env bash
# Check that adl-lsp is fit to release and, with --publish, release it.
#
# Without --publish nothing leaves this machine: the script formats, lints, tests and does a
# dry run of packaging the crate.
#
# With --publish it also:
#   - publishes the crate to crates.io (`cargo install adl-lsp` builds it in release mode)
#   - tags the commit adl-lsp-<version> and pushes the tag, which starts the GitHub workflow
#     that builds the release binaries
#
# usage: scripts/release-server.sh [--publish]
set -euo pipefail

publish=false
for arg in "$@"; do
  case "$arg" in
    --publish) publish=true ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root/rust/adl-lsp"

fail() { echo "✘ $*" >&2; exit 1; }
ok() { echo "✔ $*"; }

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
tag="adl-lsp-$version"
echo "Releasing adl-lsp $version"

status=$(curl -s -o /dev/null -w "%{http_code}" -A "adl-lsp release script" \
  "https://crates.io/api/v1/crates/adl-lsp/$version" || true)
[ "$status" != "200" ] || fail "adl-lsp $version is already on crates.io; bump the version in Cargo.toml"
ok "adl-lsp $version is not yet on crates.io"

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  fail "tag $tag already exists"
fi
ok "tag $tag is free"

dirty=$(git status --porcelain -- "$root")
if [ -n "$dirty" ]; then
  $publish && fail "the working tree has uncommitted changes:
$dirty"
  echo "! the working tree has uncommitted changes; commit them before publishing"
fi

cargo fmt --check
ok "formatted"
cargo clippy --all-targets --quiet -- -D warnings
ok "no clippy warnings"
cargo test --quiet
ok "tests pass"

# Packages the crate and builds it from the package, so files missing from the published
# crate (such as the bundled standard library) are caught here.
if $publish; then
  cargo publish --dry-run --quiet
else
  cargo publish --dry-run --quiet --allow-dirty
fi
ok "crate packages and builds"

if ! $publish; then
  echo
  echo "All checks passed. Nothing was published; run with --publish to release."
  exit 0
fi

cargo publish
git tag -a -m "$tag" "$tag"
git push origin "$tag"

cat <<NEXT

Published adl-lsp $version.

Next:
  1. Watch the release build: https://github.com/alexytsu/adl-lsp/actions
     then publish the draft:  https://github.com/alexytsu/adl-lsp/releases
  2. If the extension needs this version, raise the minimum in
     ts/adl-vscode/src/check-version.ts and run scripts/package-extension.sh
NEXT
