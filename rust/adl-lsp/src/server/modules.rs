use async_lsp::lsp_types::Url;
use std::path::Path;
use tracing::debug;

use crate::parser::definition::UnresolvedImport;

pub fn resolve_import(source_uri: &Url, unresolved_import: &UnresolvedImport) -> Option<Url> {
    debug!(
        "Resolving import: source={:?}, target_path={:?}, identifier={}",
        source_uri.path(),
        unresolved_import.target_module_path,
        unresolved_import.identifier
    );

    // TODO: get the adl roots from the `workspace_directories` config 
    // We can only resolve imports within the same adl location via this technique so all adl files must be colocated
    // However, to resolve the adl standard library and `sys` modules as well as other modules (such as those from `helix-core`) we'd need
    // to be aware of other adl roots

    // Get the root of the adl workspace
    let source_path = Path::new(source_uri.path());
    let source_module_path: Vec<&str> = unresolved_import.source_module.split(".").collect();
    let source_module_depth = source_module_path.len();
    let adl_root = source_path
        .ancestors()
        .nth(source_module_depth)
        .unwrap();
    debug!("ADL root: {:?}", adl_root);

    // Resolve module path to file path
    let target_path = adl_root.join(format!(
        "{}.adl",
        unresolved_import.target_module_path.join("/")
    ));

    debug!("Resolved path: {:?}", target_path);

    Url::from_file_path(target_path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_import_in_shared_module() {
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "common.main".into(),
            target_module_path: vec!["common".into(), "strings".into()],
            identifier: "StringML".into(),
        };

        let resolved = resolve_import(&source_uri, &unresolved).unwrap();
        assert_eq!(
            resolved,
            Url::parse("file:///project/adl/common/strings.adl").unwrap(),
        );
    }

    #[test]
    fn test_resolve_import_in_distinct_module() {
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "common.main".into(),
            target_module_path: vec!["app".into(), "main".into()],
            identifier: "User".into(),
        };

        let resolved = resolve_import(&source_uri, &unresolved).unwrap();
        assert_eq!(
            resolved,
            Url::parse("file:///project/adl/app/main.adl").unwrap()
        );
    }

    #[test]
    fn test_deeply_nested_import() {
        let source_uri = Url::parse("file:///project/adl/a/b/c/d/e/f/g/module.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "a.b.c.d.e.f.g.module".into(),
            target_module_path: vec![
                "a".into(),
                "b".into(),
                "c".into(),
                "d".into(),
                "e".into(),
                "ff".into(),
                "gg".into(),
                "hh".into(),
                "ii".into(),
                "module".into(),
            ],
            identifier: "StringML".into(),
        };

        let resolved = resolve_import(&source_uri, &unresolved).unwrap();
        assert_eq!(
            resolved,
            Url::parse("file:///project/adl/a/b/c/d/e/ff/gg/hh/ii/module.adl").unwrap()
        );
    }

    #[test]
    fn test_rooted_deep_in_workspace() {
        let source_uri =
            Url::parse("file:///project/a/b/c/d/e/f/g/adl/a/b/c/d/e/f/g/module.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "a.b.c.d.e.f.g.module".into(),
            target_module_path: vec![
                "a".into(),
                "b".into(),
                "c".into(),
                "d".into(),
                "e".into(),
                "ff".into(),
                "gg".into(),
                "hh".into(),
                "ii".into(),
                "module".into(),
            ],
            identifier: "StringML".into(),
        };

        let resolved = resolve_import(&source_uri, &unresolved).unwrap();
        assert_eq!(
            resolved,
            Url::parse("file:///project/a/b/c/d/e/f/g/adl/a/b/c/d/e/ff/gg/hh/ii/module.adl")
                .unwrap()
        );
    }
}
