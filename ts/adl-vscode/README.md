# adl-vscode

This provides Language Server Protocol client capabilities integrated with
[adl-lsp](https://github.com/alexytsu/adl-lsp). It is published as a
[VSCode extension](https://marketplace.visualstudio.com/items?itemName=alexytsu.adl-vscode).

## Features

- ✅ Syntax highlighting (by [guyNeara](https://github.com/guyNeara) from
  [adl-vscode-highlight](https://github.com/adl-lang/adl-vscode-highlight))
- ✅ Goto definition and goto references
- ✅ Diagnostics
- ✅ Hover information

Further planned features

- 🚧 Symbol renaming
- 🚧 Import management
- 🚧 Code completion and suggestions
- 🚧 Formatting
- 🚧 Style and linting rules
- 🚧 Type-checking of interior JSON values

[CHANGELOG](https://marketplace.visualstudio.com/items/alexytsu.adl-vscode/changelog)

## Requirements

You will need to install [adl-lsp](https://github.com/alexytsu/adl-lsp) and have
it on your path. See the
[README](https://github.com/alexytsu/adl-lsp?tab=readme-ov-file#installation)
for instructions to install.

## Extension Settings

This extension contributes the following settings:

- `adl.lspPath`: If the `adl-lsp` is not available on your default path, specify
  its location here.
- `adl.searchDirs`: ADL package locations. An ADL package is the directory that
  contains top-level ADL modules and may contain an `adl-package.json` file that
  specifies dependencies.

## Publishing checklist

The extension is uploaded to the marketplace by hand as a `.vsix` file.

1. If the extension needs a new `adl-lsp`, publish that first (see the
   [server README](../../rust/adl-lsp/README.md#publishing-checklist)) and raise
   the minimum version in [check-version.ts](./src/check-version.ts)
2. Update the version number in `package.json` and add an entry to the
   [changelog](./CHANGELOG.md)
3. `git commit -am "adl-vscode version ${SEMVER}"`
4. Run `npm run vsix` (or `scripts/package-extension.sh` from the repository
   root). It builds `adl-vscode-${SEMVER}.vsix` and refuses to continue unless
   the package is a minified production build without sources or source maps,
   the changelog has an entry for the version, and the required `adl-lsp`
   version is on crates.io
5. Upload the `.vsix` at
   <https://marketplace.visualstudio.com/manage/publishers/alexytsu>
6. `git tag -a -m "adl-vscode-${SEMVER}" adl-vscode-${SEMVER}` and push the tag
