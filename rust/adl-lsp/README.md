# ADL Language Server (Rust Implementation)

A Rust implementation of a Language Server Protocol (LSP) server for [Algebraic Data Language](https://github.com/adl-lang/adl).

## Overview

This crate implements a language server that provides IDE features for ADL files through the [Language Server Protocol](https://microsoft.github.io/language-server-protocol/). It uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) for efficient parsing, with the grammar defined in [tree-sitter-adl](https://github.com/alexytsu/tree-sitter-adl).

## Features

- Go to definition
- Diagnostics and error reporting
- Hover information
- Code completion
- Import resolution and management

## Usage

This crate is primarily used as a library by the VSCode extension. For development:

```bash
cargo build
cargo test
```

## License

MIT License
