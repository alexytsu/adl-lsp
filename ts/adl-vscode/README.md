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
it on your path. The easiest way to do this is `cargo install adl-lsp`.

## Extension Settings

This extension contributes the following settings:

- `adl.lspPath`: If you ran `cargo install adl-lsp` set this to
  "~/.cargo/bin/adl-lsp"
- `adl.searchDirs`: ADL package locations. An ADL package is the directory
  that contains top-level ADL modules.

## Publishing checklist

- Update the version number in `package.json`
- Update the [changelog](./CHANGELOG.md)
- Update the minimum `adl-lsp` version number in
  [check-version.ts](./src/check-version.ts) if necessary
- `vsce publish`
- `git tag -a -f -m "adl-vscode-${SEMVER}" adl-vscode-${SEMVER}`
