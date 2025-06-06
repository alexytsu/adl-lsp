# ADL Language Server

This repo contains an implementation of a language server for
[Algebraic Data Language](https://github.com/adl-lang/adl) and a VSCode
extension implementing the corresponding language client.

## Language Server

The language server conforms to the
[Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
and uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) to implement
efficient parsing. The full grammar is defined in
[tree-sitter-adl](https://github.com/adl-lang/tree-sitter-adl).

The Rust code in `adl-lsp` borrows code and architecture from
[coder3101's](https://github.com/coder3101) implementation of a
[Protols](https://github.com/coder3101/protols) (a Protobuf Language Server).

## ✨ Features

When paired with the VSCode extension you will be provided

- ✅ Syntax highlighting
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

## License

This project is licensed under the MIT License.