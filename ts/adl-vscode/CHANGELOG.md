# Change Log

All notable changes to the "alexytsu.adl-vscode" extension will be documented in
this file.

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
