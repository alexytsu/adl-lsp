use async_lsp::lsp_types::Url;
use std::path::{Path, PathBuf};
use tracing::trace;

use crate::parser::definition::UnresolvedImport;

pub fn resolve_import(
    package_roots: &[PathBuf],
    source_uri: &Url,
    unresolved_import: &UnresolvedImport,
) -> Vec<Url> {
    // the source package might not be specified in the roots, we can be lenient of that
    let mut potential_urls = Vec::with_capacity(package_roots.len() + 1);
    trace!(
        "resolving import: source={:?}, target_path={:?}, identifier={}",
        source_uri.path(),
        unresolved_import.target_module_path,
        unresolved_import.identifier
    );

    // Get the root of the package that contains the source module
    let source_path = Path::new(source_uri.path());
    let source_module_path: Vec<&str> = unresolved_import.source_module.split(".").collect();
    let source_module_depth = source_module_path.len();
    let adl_root = source_path.ancestors().nth(source_module_depth).unwrap();

    // NOTE: could send a notification for diagnostics to report if the source module *isn't in the specified workspace roots

    // Prioritise resolving to the source package (most likely here)
    let source_package_target_path = adl_root.join(format!(
        "{}.adl",
        unresolved_import.target_module_path.join("/")
    ));

    potential_urls.push(Url::from_file_path(&source_package_target_path).unwrap());

    potential_urls.extend(package_roots.iter().filter_map(|root| {
        let target_path = root.join(format!(
            "{}.adl",
            unresolved_import.target_module_path.join("/")
        ));
        if target_path == source_package_target_path {
            return None;
        }
        Url::from_file_path(target_path).ok()
    }));

    potential_urls.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_import_in_shared_module() {
        let package_roots = vec![PathBuf::from("/project/adl")];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "common.main".into(),
            target_module_path: vec!["common".into(), "strings".into()],
            identifier: "StringML".into(),
        };

        let resolved = resolve_import(&package_roots, &source_uri, &unresolved);
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved.first().unwrap(),
            &Url::parse("file:///project/adl/common/strings.adl").unwrap(),
        );
    }

    #[test]
    fn test_resolve_import_in_same_package_no_root_specified() {
        let package_roots = vec![];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "common.main".into(),
            target_module_path: vec!["common".into(), "strings".into()],
            identifier: "StringML".into(),
        };

        let resolved = resolve_import(&package_roots, &source_uri, &unresolved);
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved.first().unwrap(),
            &Url::parse("file:///project/adl/common/strings.adl").unwrap(),
        );
    }

    #[test]
    fn test_resolve_import_in_distinct_module() {
        let package_roots = vec![PathBuf::from("/project/adl")];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "common.main".into(),
            target_module_path: vec!["app".into(), "main".into()],
            identifier: "User".into(),
        };

        let resolved = resolve_import(&package_roots, &source_uri, &unresolved);
        assert_eq!(
            resolved.first().unwrap(),
            &Url::parse("file:///project/adl/app/main.adl").unwrap(),
        );
    }

    #[test]
    fn test_deeply_nested_import() {
        let package_roots = vec![PathBuf::from("/project/adl")];
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

        let resolved = resolve_import(&package_roots, &source_uri, &unresolved);
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved.first().unwrap(),
            &Url::parse("file:///project/adl/a/b/c/d/e/ff/gg/hh/ii/module.adl").unwrap(),
        );
    }

    #[test]
    fn test_rooted_deep_in_workspace() {
        let package_roots = vec![PathBuf::from("/project/a/b/c/d/e/f/g/adl")];
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

        let resolved = resolve_import(&package_roots, &source_uri, &unresolved);
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved.first().unwrap(),
            &Url::parse("file:///project/a/b/c/d/e/f/g/adl/a/b/c/d/e/ff/gg/hh/ii/module.adl")
                .unwrap()
        );
    }

    #[test]
    fn test_multiple_possible_imports() {
        let package_roots = vec![
            PathBuf::from("/project/adl2"),
            PathBuf::from("/another-project/adl"),
            PathBuf::from("/project/adl"),
        ];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let unresolved = UnresolvedImport {
            source_module: "common.main".into(),
            target_module_path: vec!["common".into(), "strings".into()],
            identifier: "StringML".into(),
        };

        let resolved = resolve_import(&package_roots, &source_uri, &unresolved);
        assert_eq!(resolved.len(), 3);
        assert!(resolved.contains(&Url::parse("file:///project/adl/common/strings.adl").unwrap()));
        assert!(resolved.contains(&Url::parse("file:///project/adl2/common/strings.adl").unwrap()));
        assert!(
            resolved
                .contains(&Url::parse("file:///another-project/adl/common/strings.adl").unwrap())
        );

        // takes precedence
        assert_eq!(
            resolved.first().unwrap(),
            &Url::parse("file:///project/adl/common/strings.adl").unwrap()
        );
    }
}
