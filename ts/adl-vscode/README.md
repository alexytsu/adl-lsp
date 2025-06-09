# adl-vscode

This provides Language Server Protocol client capabilities integrated with
[adl-lsp](https://github.com/alexytsu/adl-lsp). It is published as a
[VSCode extension](https://marketplace.visualstudio.com/items?itemName=alexytsu.adl-vscode).

## Features

- ✅ Syntax highlighting (by [guyNeara](https://github.com/guyNeara) from
  [adl-vscode-highlight](https://github.com/adl-lang/adl-vscode-highlight))
- ✅ Go to definition
- ✅ Diagnostics
- ✅ Hover information

Further planned features

- 🚧 Code completion
- 🚧 Formatting
- 🚧 Symbol renaming
- 🚧 Import management
- 🚧 Type-checking of interior JSON values
- 🚧 Plugins for other editors (neovim, helix)

[CHANGELOG](https://marketplace.visualstudio.com/items/alexytsu.adl-vscode/changelog)

## Requirements

You will need to install [adl-lsp](https://github.com/alexytsu/adl-lsp) and have
it on your path. The easiest way to do this is `cargo install adl-lsp`.

## Extension Settings

This extension contributes the following settings:

- `adl.lspPath`: If you ran `cargo install adl-lsp` set this to
  "~/.cargo/bin/adl-lsp"
- `adl.packageRoots`: ADL package locations. An ADL package is the directory
  that contains top-level ADL modules.

## Publishing

- Update the [changelog](./CHANGELOG.md) and version number in `package.json`
- `vsce publish`
