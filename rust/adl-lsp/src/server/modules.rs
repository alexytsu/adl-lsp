use async_lsp::lsp_types::Url;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tracing::trace;

#[derive(Debug, Deserialize)]
pub struct AdlPackageRef {
    pub localdir: String,
}

/// ADL package definition JSON schema
/// NOTE(alex): this will need to be updated if AdlPackageRef is extended
/// ```adl
///module adlc.package {
///struct AdlPackage {
///    String name;
///    Vector<AdlPackageRef> dependencies = [];
///};
///
///union AdlPackageRef {
///    String localdir;
///};
///};
/// ```
///
#[derive(Debug, Deserialize)]
pub struct AdlPackageDefinition {
    pub name: String,
    pub dependencies: Vec<AdlPackageRef>,
}


pub fn resolve_import(
    search_dirs: &[PathBuf],
    source_uri: &Url,
    source_module: &str,
    imported_module_path: &Vec<&str>,
    document_exists: &impl Fn(&PathBuf) -> bool, // assuming that if a .adl file exists here, it is valid
) -> Option<Url> {
    trace!(
        "resolving import: source={:?}, imported_module_path={:?}",
        source_uri.path(),
        imported_module_path,
    );

    // Get the root of the package that contains the source module
    let source_path = Path::new(source_uri.path());
    let source_module_path: Vec<&str> = source_module.split(".").collect();
    let source_module_depth = source_module_path.len();
    let adl_root = source_path.ancestors().nth(source_module_depth);
    let source_package_target_path =
        adl_root.map(|adl| adl.join(format!("{}.adl", imported_module_path.join("/"))));

    // Prioritise resolving to the source package (most likely here)
    if let Some(ref source_package_target_path) = source_package_target_path {
        if document_exists(source_package_target_path) {
            return Some(
                Url::from_file_path(source_package_target_path).expect("invalid file path"),
            );
        }
    }

    // Check if the source package is in other package roots
    for root in search_dirs {
        let target_path = root.join(format!("{}.adl", imported_module_path.join("/")));
        if let Some(ref source_package_target_path) = source_package_target_path {
            if &target_path == source_package_target_path {
                continue;
            }
        }
        if document_exists(&target_path) {
            return Some(Url::from_file_path(&target_path).expect("invalid file path"));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_import_in_same_package_sibling() {
        let search_dirs = vec![PathBuf::from("/project/adl")];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();

        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "common.main",
            &vec!["common", "strings"],
            &|_| true,
        );
        assert_eq!(
            resolved,
            Some(Url::parse("file:///project/adl/common/strings.adl").unwrap())
        );
    }

    #[test]
    fn test_resolve_import_in_same_package_sibling_implicit() {
        // no package roots, rely on implicit resolution in same package root
        let search_dirs = vec![];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();

        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "common.main",
            &vec!["common", "strings"],
            &|_| true,
        );
        assert_eq!(
            resolved,
            Some(Url::parse("file:///project/adl/common/strings.adl").unwrap()),
        );
    }

    #[test]
    fn test_resolve_import_in_same_package_cousin_implicit() {
        let search_dirs = vec![];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();

        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "common.main",
            &vec!["app", "main"],
            &|_| true,
        );
        assert_eq!(
            resolved,
            Some(Url::parse("file:///project/adl/app/main.adl").unwrap()),
        );
    }

    #[test]
    fn test_deeply_nested_import() {
        let search_dirs = vec![PathBuf::from("/project/adl")];
        let source_uri = Url::parse("file:///project/adl/a/b/c/d/e/f/g/module.adl").unwrap();
        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "a.b.c.d.e.f.g.module",
            &vec!["a", "b", "c", "d", "e", "ff", "gg", "hh", "ii", "module"],
            &|_| true,
        );
        assert_eq!(
            resolved,
            Some(Url::parse("file:///project/adl/a/b/c/d/e/ff/gg/hh/ii/module.adl").unwrap())
        );
    }

    #[test]
    fn test_rooted_deep_in_workspace() {
        let search_dirs = vec![PathBuf::from("/project/a/b/c/d/e/f/g/adl")];
        let source_uri =
            Url::parse("file:///project/a/b/c/d/e/f/g/adl/a/b/c/d/e/f/g/module.adl").unwrap();
        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "a.b.c.d.e.f.g.module",
            &vec!["a", "b", "c", "d", "e", "ff", "gg", "hh", "ii", "module"],
            &|_| true,
        );
        assert_eq!(
            resolved,
            Some(
                Url::parse("file:///project/a/b/c/d/e/f/g/adl/a/b/c/d/e/ff/gg/hh/ii/module.adl")
                    .unwrap()
            )
        );
    }

    #[test]
    fn test_resolve_from_other_package_root() {
        let search_dirs = vec![
            PathBuf::from("/project/adl"),
            PathBuf::from("/project/adl-no-strings"),
            PathBuf::from("/project/adl-strings"),
        ];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "common.main",
            &vec!["common", "strings"],
            &|path| path.starts_with("/project/adl-strings"),
        );
        assert_eq!(
            resolved,
            Some(Url::parse("file:///project/adl-strings/common/strings.adl").unwrap())
        );
    }
}
