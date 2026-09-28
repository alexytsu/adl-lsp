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

## Command line

```
adl-lsp [--search-dirs <DIR>,...] [--stdlib-dir <DIR>] [--log-level <LEVEL>]
```

- `--search-dirs`: directories to search for ADL files. Package roots inside them
  are found from `adl-package.json` files and from module names.
- `--stdlib-dir` (or `ADL_LSP_STDLIB_DIR`): see below.
- `--log-level` (or `ADL_LSP_LOG_LEVEL`): `error`, `warn`, `info` (the default),
  `debug` or `trace`. Logs are written to stderr.

## The ADL standard library

Modules such as `sys.types` are installed with the ADL compiler rather than kept
in each workspace. The server uses the first standard library it finds in:

1. `--stdlib-dir`, the directory that contains `sys/types.adl`
2. `$ADL_ROOT/lib/adl`, the override that `adlc` itself honours
3. a toolchain installed in the workspace: `.local/lib/adl` in a search dir, in
   one of its parent directories up to the repository root, or one directory
   below any of those (for example `deno/.local/lib/adl`)
4. `../lib/adl` relative to the `adlc` executable on `PATH`
5. the directory printed by `adlc show --adlstdlib`, which covers version manager
   shims and cabal or nix installs
6. the newest per-user install: `~/.cache/adl/<version>` or
   `~/Library/Caches/adl/<version>` (the wrapper script from the ADL install
   guide), and `tools/adlc/<version>` under `$PROTO_HOME`, `~/.proto` or
   `~/.config/proto` ([proto](https://moonrepo.dev/proto))
7. a copy bundled with the server (from [`stdlib`](./stdlib)), which is written
   to the user cache directory

A workspace that contains its own `sys` modules always takes priority over all
of these. The chosen location is logged at startup.

## License

MIT License. The bundled ADL standard library in [`stdlib`](./stdlib) is
distributed under its own [BSD 3-Clause license](./stdlib/LICENSE).

## Publishing checklist

- Update the version number in `Cargo.toml`
- Release a new version of `adl-vscode` with a new minimum `adl-lsp` version requirement if necessary
- `cargo publish`
- `git tag -a -f -m "adl-lsp-${SEMVER}" adl-lsp-${SEMVER}`
