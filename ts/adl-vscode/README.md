# adl-vscode

This provides Language Server Protocol client capabilities integrated with
[adl-lsp](https://github.com/alexytsu/adl-lsp)

## Features

- ✅ Syntax highlighting (by [guyNeara](https://github.com/guyNeara) from
  [adl-vscode-highlight](https://github.com/adl-lang/adl-vscode-highlight))
- ✅ Go to definition
- ✅ Diagnostics
- ✅ Hover information
- ✅ Code completion
- ✅ Formatting

Further planned features

- 🚧 Symbol renaming
- 🚧 Import management
- 🚧 Type-checking of interior JSON values
- 🚧 Plugins for other editors (neovim, helix)

## Requirements

You will need to install [adl-ls](https://github.com/alexytsu/adl-lsp) and have
it on your path. The easiest way to do this is `cargo install adl-lsp`.

## Extension Settings

This extension contributes the following settings:

- `adl.lspPath`: If you ran `cargo install adl-lsp` set this to
  "~/.cargo/bin/adl-lsp"

## Known Issues

## Release Notes
