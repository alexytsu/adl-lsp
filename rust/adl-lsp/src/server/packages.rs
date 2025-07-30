use async_lsp::lsp_types::Url;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use tracing::{error, trace};

#[derive(Debug, Deserialize)]
pub struct AdlPackageRef {
    pub localdir: String,
}

/// Find the package root by looking up the directory tree for a file named `adl-package.json`
pub fn find_package_root_by_marker<T: AsRef<Path>>(path: T) -> Option<PathBuf> {
    let path = path.as_ref();
    if !path.exists() {
        None
    } else if path.is_dir() && path.join("adl-package.json").exists() {
        Some(path.to_path_buf())
    } else {
        path.parent().and_then(find_package_root_by_marker)
    }
}

/// Resolve a dependency path, handling both relative and absolute paths
pub fn resolve_dependency_path<T: AsRef<Path>>(package_root: T, localdir: &str) -> PathBuf {
    // Check if it's an absolute path
    if localdir.starts_with('/') {
        PathBuf::from(localdir)
    } else {
        // Treat as relative path from package root
        package_root.as_ref().join(localdir)
    }
}

/// Normalize a path by resolving all relative components
pub fn normalize_path<T: AsRef<Path>>(path: T) -> PathBuf {
    path.as_ref()
        .canonicalize()
        .unwrap_or_else(|_| path.as_ref().to_path_buf())
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
    #[allow(dead_code)]
    pub name: String,
    pub dependencies: Vec<AdlPackageRef>,
}

pub fn resolve_import(
    search_dirs: &HashMap<PathBuf, HashSet<Url>>,
    // TODO(med): don't need these parameters if we trust fully in the search dirs being passed in
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


    // Check if the source package is in other package roots
    for (package_root, adl_files) in search_dirs {
        let target_path = package_root.join(format!("{}.adl", imported_module_path.join("/")));
        let target_uri = Url::from_file_path(&target_path);
        if let Ok(target_uri) = target_uri {
            if adl_files.contains(&target_uri) {
                return Some(target_uri);
            }
        }
    }

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

    // NOTE(alex): the below checks are redundant as the lookup above should always succeed
    for package_root in search_dirs.keys() {
        let target_path = package_root.join(format!("{}.adl", imported_module_path.join("/")));
        if let Some(ref source_package_target_path) = source_package_target_path {
            if &target_path == source_package_target_path {
                continue;
            }
        }
        if document_exists(&target_path) {
            error!("found target path: {} on disk but wasn't found in the search_dir cache", target_path.display());
            return Some(Url::from_file_path(&target_path).expect("invalid file path"));
        }
    }

    None
}

// TODO(low): the below tests rely on finding the files on disk, assuming a cache miss
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_import_in_same_package_sibling() {
        let search_dirs = HashMap::from([(PathBuf::from("/project/adl"), HashSet::from([]))]);
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
        let search_dirs = HashMap::from([]);
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
        let search_dirs = HashMap::from([]);
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
        let search_dirs = HashMap::from([(PathBuf::from("/project/adl"), HashSet::from([]))]);
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
        let search_dirs = HashMap::from([(PathBuf::from("/project/a/b/c/d/e/f/g/adl"), HashSet::from([]))]);
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
        let search_dirs = HashMap::from([
            (PathBuf::from("/project/adl"), HashSet::from([])),
            (PathBuf::from("/project/adl-no-strings"), HashSet::from([])),
            (PathBuf::from("/project/adl-strings"), HashSet::from([])),
        ]);
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
