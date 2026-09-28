# ADL standard library (vendored)

These files are an unmodified copy of `adl/stdlib` from
[adl-lang/adl](https://github.com/adl-lang/adl). They are distributed under the
BSD 3-Clause license in [LICENSE](./LICENSE).

`adl-lsp` embeds them in the binary so that imports of `sys.*` and `adlc.*`
modules resolve in workspaces that do not vendor the standard library
themselves. See `src/server/stdlib.rs`.

To refresh them, copy `adl/stdlib` from upstream over this directory and update
the file list in `src/server/stdlib.rs`.
