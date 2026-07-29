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

/// Derive a package root from a file's path and its declared module name.
///
/// A module `a.b.c` declared in `<root>/a/b/c.adl` implies the package root is `<root>`, reached by
/// walking up `module_name.split('.').count()` ancestors from the file. This is a fallback for
/// workspaces without an `adl-package.json` marker (see [`find_package_root_by_marker`]).
pub fn package_root_from_module<T: AsRef<Path>>(
    file_path: T,
    module_name: &str,
) -> Option<PathBuf> {
    if module_name.is_empty() {
        return None;
    }
    let module_depth = module_name.split('.').count();
    file_path
        .as_ref()
        .ancestors()
        .nth(module_depth)
        .map(Path::to_path_buf)
}

/// Resolve a dependency path, handling both relative and absolute paths
pub fn resolve_dependency_path<T: AsRef<Path>>(package_root: T, localdir: &str) -> PathBuf {
    // Check if it's an absolute path
    if Path::new(localdir).is_absolute() {
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
    // `source_uri` and `source_module` are retained (not redundant): they let us derive the
    // source package root by walking up `source_module` ancestors so imports resolve to the
    // *source* package first. `document_exists` supports the implicit-resolution path used when
    // `search_dirs` is empty (e.g. single-file / marker-less workspaces).
    source_uri: &Url,
    source_module: &str,
    imported_module_path: &[&str],
    document_exists: &impl Fn(&Path) -> bool, // assuming that if a .adl file exists here, it is valid
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

    // Check other package roots only after attempting the source package
    let mut package_roots: Vec<&PathBuf> = search_dirs.keys().collect();
    package_roots.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));

    for package_root in &package_roots {
        let target_path = package_root.join(format!("{}.adl", imported_module_path.join("/")));
        if let Some(ref source_package_target_path) = source_package_target_path {
            if &target_path == source_package_target_path {
                continue;
            }
        }
        let target_uri = Url::from_file_path(&target_path);
        if let Ok(target_uri) = target_uri {
            // Prefer the discovered-files cache when it is populated.
            if let Some(adl_files) = search_dirs.get(*package_root) {
                if adl_files.contains(&target_uri) {
                    return Some(target_uri);
                }
            }
            // Fall back to a filesystem probe. This covers marker-less workspaces where the
            // cache is empty, and files discovered after the initial workspace scan.
            if document_exists(&target_path) {
                if search_dirs
                    .get(*package_root)
                    .is_some_and(|f| !f.is_empty())
                {
                    error!(
                        "found target path: {} on disk but wasn't found in the search_dir cache",
                        target_path.display()
                    );
                }
                return Some(target_uri);
            }
        }
    }

    None
}

// Most tests below drive the filesystem-probe fallback via `document_exists`, simulating a
// marker-less workspace with an empty discovery cache. `test_resolve_from_other_package_root_cached`
// instead exercises the populated-cache path.
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
            &["common", "strings"],
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
            &["common", "strings"],
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
            &["app", "main"],
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
            &["a", "b", "c", "d", "e", "ff", "gg", "hh", "ii", "module"],
            &|_| true,
        );
        assert_eq!(
            resolved,
            Some(Url::parse("file:///project/adl/a/b/c/d/e/ff/gg/hh/ii/module.adl").unwrap())
        );
    }

    #[test]
    fn test_rooted_deep_in_workspace() {
        let search_dirs = HashMap::from([(
            PathBuf::from("/project/a/b/c/d/e/f/g/adl"),
            HashSet::from([]),
        )]);
        let source_uri =
            Url::parse("file:///project/a/b/c/d/e/f/g/adl/a/b/c/d/e/f/g/module.adl").unwrap();
        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "a.b.c.d.e.f.g.module",
            &["a", "b", "c", "d", "e", "ff", "gg", "hh", "ii", "module"],
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
            &["common", "strings"],
            &|path| path.starts_with("/project/adl-strings"),
        );
        assert_eq!(
            resolved,
            Some(Url::parse("file:///project/adl-strings/common/strings.adl").unwrap())
        );
    }

    #[test]
    fn test_resolve_from_other_package_root_cached() {
        // The discovery cache for `adl-strings` is populated with the target file, so resolution
        // succeeds via the cache without any filesystem probe (`document_exists` always false).
        let target_uri = Url::parse("file:///project/adl-strings/common/strings.adl").unwrap();
        let search_dirs = HashMap::from([
            (PathBuf::from("/project/adl"), HashSet::from([])),
            (PathBuf::from("/project/adl-no-strings"), HashSet::from([])),
            (
                PathBuf::from("/project/adl-strings"),
                HashSet::from([target_uri.clone()]),
            ),
        ]);
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let resolved = resolve_import(
            &search_dirs,
            &source_uri,
            "common.main",
            &["common", "strings"],
            &|_| false,
        );
        assert_eq!(resolved, Some(target_uri));
    }
}
