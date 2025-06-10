# ADL Language Server

This repo contains an implementation of a language server for
[Algebraic Data Language](https://github.com/adl-lang/adl) and a VSCode
extension implementing the corresponding language client.

## Language Server

The language server conforms to the
[Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
and uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) to implement
efficient parsing. The full grammar is defined in
[tree-sitter-adl](https://github.com/alexytsu/tree-sitter-adl).

The Rust code in `adl-lsp` borrows code and architecture from
[coder3101's](https://github.com/coder3101) implementation of 
[Protols](https://github.com/coder3101/protols) (a Protobuf Language Server).

## ✨ Features

When paired with the VSCode extension you will be provided

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
- 🚧 Plugins for other editors (neovim, helix)

## Editor Support

### VSCode

This repo implements a VSCode client extension that is published to the [marketplace](https://marketplace.visualstudio.com/items?itemName=alexytsu.adl-vscode). See the [README](./ts/adl-vscode/README.md) for futher configuration instructions.

### Helix

```toml
[[language]]
name = "algebraic-data-language"
scope = "source.adl"
language-id = "adl"
file-types = ["adl", "adl-cpp", "adl-hs", "adl-java", "adl-hs", "adl-rs", "adl-ts"]
roots = [".git"]
comment-tokens = ["//", "///", "//!"]
language-servers = ["adl-lsp"]
workspace-lsp-roots = ["adl"]

[language-server.adl-lsp]
command = "adl-lsp"
```

### Vim

First install [adl-vim-highlight] for ADL file detection and syntax highlighting

```lua
vim.lsp.config["adl-lsp"] = {
  cmd = { "adl-lsp" },
  filetypes = { "adl" },
  root_markers = { ".git" },
}

vim.lsp.enable("adl-lsp")
```

## License

This project is licensed under the MIT License.
