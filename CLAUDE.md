# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This repository contains an ADL (Algebraic Data Language) Language Server Protocol implementation and VSCode extension. The project is split into two main components:

- **Rust Language Server** (`rust/adl-lsp/`): LSP server implementation using tree-sitter for parsing
- **TypeScript VSCode Extension** (`ts/adl-vscode/`): Language client that communicates with the LSP server

The language server uses [tree-sitter-adl](https://github.com/alexytsu/tree-sitter-adl) for grammar parsing and borrows architecture from [Protols](https://github.com/coder3101/protols).

## Development Commands

### Rust Language Server (`rust/adl-lsp/`)

```bash
# Build the language server
cd rust/adl-lsp
cargo build

# Run tests with snapshot testing
cargo test

# Format code
cargo fmt

# Lint code
cargo clippy
```

### VSCode Extension (`ts/adl-vscode/`)

```bash
# Install dependencies
cd ts/adl-vscode
npm install

# Build for development
npm run compile

# Build for production
npm run package

# Run linting
npm run lint

# Type checking
npm run check-types

# Run tests
npm run test

# Watch mode for development
npm run watch
```

## Architecture

### Language Server Structure

- `src/main.rs`: Entry point and CLI argument parsing
- `src/server/`: LSP server implementation and configuration
- `src/parser/`: Tree-sitter integration, diagnostics, hover, and go-to-definition features
- `src/node.rs`: Tree-sitter node utilities

### VSCode Extension Structure

- `src/extension.ts`: Main extension entry point that starts the language client
- `dist/`: Built extension output (created by esbuild)
- `syntaxes/adl.tmLanguage.json`: TextMate grammar for syntax highlighting

### Key Configuration

- The VSCode extension expects the language server binary at `~/.cargo/bin/adl-lsp` by default
- ADL package roots default to `["adl"]` but can be configured
- Uses Rust toolchain version 1.85.1 with clippy, rustfmt, and rust-analyzer components

## Testing

- Rust: Uses `insta` for snapshot testing of parser output
- TypeScript: Uses VSCode test framework with Mocha