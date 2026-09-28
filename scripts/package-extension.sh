#!/usr/bin/env bash
# Build the adl-vscode extension as a production .vsix and check it is fit to publish.
#
# The .vsix is uploaded by hand at
#   https://marketplace.visualstudio.com/manage/publishers/alexytsu
#
# usage: scripts/package-extension.sh [--allow-unpublished-server]
set -euo pipefail

allow_unpublished_server=false
for arg in "$@"; do
  case "$arg" in
    --allow-unpublished-server) allow_unpublished_server=true ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root/ts/adl-vscode"

fail() { echo "✘ $*" >&2; exit 1; }
ok() { echo "✔ $*"; }

version=$(node -p "require('./package.json').version")
vsix="adl-vscode-$version.vsix"
echo "Packaging adl-vscode $version"

# --- checks before building -------------------------------------------------

grep -q "^## \[$version\]" CHANGELOG.md \
  || fail "CHANGELOG.md has no entry for $version"
ok "changelog has an entry for $version"

required_server=$(node -e '
  const src = require("fs").readFileSync("src/check-version.ts", "utf8");
  const part = (name) => src.match(new RegExp(`REQUIRED_${name}_VERSION = (\\d+)`))[1];
  console.log(["MAJOR", "MINOR", "PATCH"].map(part).join("."));
')
status=$(curl -s -o /dev/null -w "%{http_code}" -A "adl-lsp release script" \
  "https://crates.io/api/v1/crates/adl-lsp/$required_server" || true)
if [ "$status" = "200" ]; then
  ok "required server adl-lsp $required_server is published on crates.io"
elif $allow_unpublished_server; then
  echo "! required server adl-lsp $required_server is not on crates.io (allowed by flag)"
else
  fail "this extension requires adl-lsp $required_server, which is not on crates.io (HTTP $status).
  Publish the server first (scripts/release-server.sh --publish),
  or pass --allow-unpublished-server."
fi

if [ -n "$(git status --porcelain -- . ':!*.vsix')" ]; then
  echo "! ts/adl-vscode has uncommitted changes; the package will not match a commit"
fi

# --- build --------------------------------------------------------------------

rm -rf dist "$vsix"
npm ci --silent
# `vsce package` runs the vscode:prepublish script: type check, lint, production bundle.
npx vsce package --out "$vsix"

# --- checks on what was built -------------------------------------------------

contents=$(unzip -Z1 "$vsix")
echo "$contents" | grep -qx "extension/dist/extension.js" \
  || fail "$vsix does not contain dist/extension.js"
unwanted=$(echo "$contents" | grep -E '\.map$|\.ts$|^extension/(src|node_modules|out)/' || true)
[ -z "$unwanted" ] || fail "$vsix contains development files:
$unwanted"
ok "package contains the bundle and no sources, source maps or node_modules"

bundle=$(unzip -p "$vsix" extension/dist/extension.js)
if echo "$bundle" | grep -q "sourceMappingURL"; then
  fail "the bundle references a source map: it is a development build"
fi
lines=$(echo "$bundle" | wc -l | tr -d ' ')
[ "$lines" -lt 500 ] || fail "the bundle has $lines lines: it is not minified"
ok "bundle is a minified production build ($lines lines)"

packaged_version=$(unzip -p "$vsix" extension/package.json | node -p 'JSON.parse(require("fs").readFileSync(0, "utf8")).version')
[ "$packaged_version" = "$version" ] || fail "packaged version is $packaged_version, expected $version"
ok "packaged version is $version"

cat <<NEXT

Built $root/ts/adl-vscode/$vsix

Next:
  1. Try it:    code --install-extension "$root/ts/adl-vscode/$vsix" --force
  2. Upload it: https://marketplace.visualstudio.com/manage/publishers/alexytsu
                (adl-vscode > ... > Update, then choose the .vsix)
  3. Tag it:    git tag -a -m "adl-vscode-$version" adl-vscode-$version && git push origin adl-vscode-$version
NEXT
