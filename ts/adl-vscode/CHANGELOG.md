# Change Log

All notable changes to the "alexytsu.adl-vscode" extension will be documented in
this file.

## [0.4.1] - 2026-09-28

This update requires version 0.9.1 of `adl-lsp`
- Imports from the ADL standard library (`sys.types`, `sys.adlast`, ...) resolve: hover and goto definition work on them and they are no longer reported as missing
- The standard library is taken from your installed ADL toolchain, or from a copy bundled with `adl-lsp` when none is found. Set `adl.stdlibDir` to choose one yourself
- `adl.searchDirs` defaults to the workspace folder rather than `adl`, so workspaces with several ADL directories work without configuration
- Hover and goto definition on an import that cannot be resolved return nothing rather than failing with an error
- `adl-package.json` files without a `dependencies` field are read correctly
- The server logs at `info` level rather than `debug`
- An outdated or missing `adl-lsp` is reported with a notification that stays until answered, with buttons to run or copy the update command, and a warning in the status bar
- `ADL: Restart Language Server` checks the server version again, and the new `ADL: Update Language Server` command reopens the update prompt

## [0.4.0] - 2026-09-28

This update requires version 0.9.0 of `adl-lsp`, which moves to version 0.7 of the ADL grammar
- Diagnostics update as you type rather than only on save
- Versioned declarations (e.g. `struct X#2`) are supported; goto definition prefers the highest version
- Goto definition on annotation fields resolves qualified and imported targets
- Module-form annotation declarations are parsed
- ADL keywords are reserved, which keeps parse errors local to the broken declaration
- Document symbols are named after the field rather than its type
- Installed builds always launch the `adl.lspPath` binary; the local `cargo run` server is only used in the Extension Development Host

## [0.3.0] - 2025-07-30

This [update](https://github.com/alexytsu/adl-lsp/pull/32) requires version 0.8.0 of `adl-lsp` which brings stability improvements
- Updated grammar to allow for more permissive parsing partially formed syntax
- Only attempt to parse files on save
- More errors reported for missing syntax
- Updated dark mode icon
- Better automated discovery of ADL files

## [0.2.3] - 2025-06-19

Resolves imports via fully qualified names rather than purely looking at the identifier name
Reports diagnostics for missing tokens
Reloads config when `adl.packageRoots` is updated

## [0.2.2] - 2025-06-17

Logos for ADL files

## [0.2.1] - 2025-06-17

Link to correct `adl-lsp` binary

## [0.2.0] - 2025-06-17

DO NOT INSTALL: this was mistakenly published with local development settings enabled

Features:
- Basic diagnostic errors for invalid imports: https://github.com/alexytsu/adl-lsp/issues/18
- Document outline and symbol support

Bugfixes:
- Fix parsing error for doccomments mixed with annotations
- Fix parsing error for remotely defined annotations on struct fields: https://github.com/alexytsu/adl-lsp/issues/15
- Fix goto definition for fully-qualified types: https://github.com/alexytsu/adl-lsp/issues/17

## [0.1.0] - 2025-06-11

- Implement goto references via `adl-lsp@0.5.0`
- Add support for language specific annotation files (e.g. module.adl-rs)

## [0.0.6] - 2025-06-10

- Resolve star-style imports via `adl-lsp@0.4.0`

## [0.0.5] - 2025-06-09

- Fix `adl.lspPath` homedir resolution

## [0.0.4] - 2025-06-09

- Extended VSCode compatibility range to support versions ^1.90.0

## [0.0.3] - 2025-06-09

- Added server version compatibility check
- Added `adl.packageRoots` to specify directories that can be searched to
  resolve imports

## [0.0.2] - 2025-06-09

- Added support for unresolved import handling
- Fixed hover functionality
- Support for reading `adl-lsp` from config path

## [0.0.1] - 2025-06-09

- Initial release with basic LSP functionality
- Syntax highlighting support
- Basic language server integration
