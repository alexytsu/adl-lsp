use async_lsp::lsp_types::Url;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{debug, trace, warn};

/// ADL package definition from adl-package.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdlPackageDefinition {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub dependencies: Vec<AdlPackageDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdlPackageDependency {
    pub name: String,
    pub path: String,
}

pub fn resolve_import(
    package_roots: &[PathBuf],
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
    for root in package_roots {
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

/// Discover package root by traversing up from an ADL file based on its module path
pub fn discover_package_root_from_file(file_uri: &Url, module_name: &str) -> Option<PathBuf> {
    let file_path = Path::new(file_uri.path());
    let module_parts: Vec<&str> = module_name.split('.').collect();
    let module_depth = module_parts.len();
    
    // Traverse up the directory tree by the number of module parts
    file_path.ancestors().nth(module_depth).map(|p| p.to_path_buf())
}

/// Find adl-package.json file by traversing up from a directory
pub fn find_package_definition(start_dir: &Path) -> Option<PathBuf> {
    for ancestor in start_dir.ancestors() {
        let package_file = ancestor.join("adl-package.json");
        if package_file.exists() && package_file.is_file() {
            return Some(package_file);
        }
    }
    None
}

/// Load and parse an adl-package.json file
pub fn load_package_definition(package_file: &Path) -> Result<AdlPackageDefinition, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(package_file)?;
    let package_def: AdlPackageDefinition = serde_json::from_str(&content)?;
    Ok(package_def)
}

/// Discover all package roots by examining all ADL files in initial directories
/// This function implements the automatic discovery described in the requirements:
/// 1. For each ADL file, traverse up to find the package root based on module path
/// 2. Look for adl-package.json files to discover dependencies
pub fn discover_all_package_roots(initial_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut discovered_roots = HashSet::new();
    
    debug!("Starting automatic package discovery from {} initial directories", initial_dirs.len());
    
    // If no initial directories specified, start from current working directory
    let search_dirs = if initial_dirs.is_empty() {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    } else {
        initial_dirs.to_vec()
    };
    
    for initial_dir in search_dirs {
        discover_package_roots_recursive(&initial_dir, &mut discovered_roots);
    }
    
    debug!("Discovered {} unique package roots", discovered_roots.len());
    discovered_roots.into_iter().collect()
}

/// Recursively discover package roots by examining ADL files and their module declarations
fn discover_package_roots_recursive(dir: &Path, discovered_roots: &mut HashSet<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            
            if path.is_dir() {
                // Skip hidden directories and common build/output directories
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    if !dir_name.starts_with('.')
                        && dir_name != "target"
                        && dir_name != "node_modules"
                        && dir_name != "dist"
                        && dir_name != "build"
                    {
                        discover_package_roots_recursive(&path, discovered_roots);
                    }
                }
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("adl") {
                // Found an ADL file - try to discover its package root
                if let Ok(uri) = Url::from_file_path(&path) {
                    if let Some(package_root) = discover_package_root_from_adl_file(&uri) {
                        debug!("Discovered package root from {}: {}", path.display(), package_root.display());
                        discovered_roots.insert(package_root);
                    }
                }
            }
        }
    }
}

/// Discover package root from a single ADL file by parsing its module declaration
pub fn discover_package_root_from_adl_file(file_uri: &Url) -> Option<PathBuf> {
    // Try to read the file and extract the module name
    if let Ok(content) = std::fs::read_to_string(file_uri.path()) {
        if let Some(module_name) = extract_module_name_from_content(&content) {
            return discover_package_root_from_file(file_uri, &module_name);
        }
    }
    None
}

/// Extract module name from ADL file content using simple regex-like parsing
/// This is a simplified parser - in a real implementation, you might want to use the tree-sitter parser
fn extract_module_name_from_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("module ") {
            // Extract module name: "module a.b.c;" -> "a.b.c"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let module_part = parts[1];
                // Remove trailing semicolon if present
                let module_name = module_part.trim_end_matches(';');
                return Some(module_name.to_string());
            }
        }
    }
    None
}

/// Resolve package dependencies from adl-package.json files
/// This expands the package roots to include all dependencies
pub fn resolve_package_dependencies(package_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut all_roots = HashSet::new();
    
    // Add the original package roots
    for root in package_roots {
        all_roots.insert(root.clone());
    }
    
    // For each package root, look for adl-package.json and resolve dependencies
    for root in package_roots {
        if let Some(package_file) = find_package_definition(root) {
            debug!("Found package definition at: {}", package_file.display());
            
            match load_package_definition(&package_file) {
                Ok(package_def) => {
                    debug!("Loaded package '{}' with {} dependencies", 
                          package_def.name, package_def.dependencies.len());
                    
                    let package_dir = package_file.parent().unwrap();
                    
                    for dep in &package_def.dependencies {
                        let dep_path = package_dir.join(&dep.path);
                        if dep_path.exists() && dep_path.is_dir() {
                            debug!("Adding dependency '{}' at path: {}", dep.name, dep_path.display());
                            all_roots.insert(dep_path);
                        } else {
                            warn!("Dependency '{}' path not found: {}", dep.name, dep_path.display());
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse package definition at {}: {}", package_file.display(), e);
                }
            }
        }
    }
    
    debug!("Total package roots after dependency resolution: {}", all_roots.len());
    all_roots.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_import_in_same_package_sibling() {
        let package_roots = vec![PathBuf::from("/project/adl")];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();

        let resolved = resolve_import(
            &package_roots,
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
        let package_roots = vec![];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();

        let resolved = resolve_import(
            &package_roots,
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
        let package_roots = vec![];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();

        let resolved = resolve_import(
            &package_roots,
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
        let package_roots = vec![PathBuf::from("/project/adl")];
        let source_uri = Url::parse("file:///project/adl/a/b/c/d/e/f/g/module.adl").unwrap();
        let resolved = resolve_import(
            &package_roots,
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
        let package_roots = vec![PathBuf::from("/project/a/b/c/d/e/f/g/adl")];
        let source_uri =
            Url::parse("file:///project/a/b/c/d/e/f/g/adl/a/b/c/d/e/f/g/module.adl").unwrap();
        let resolved = resolve_import(
            &package_roots,
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
        let package_roots = vec![
            PathBuf::from("/project/adl"),
            PathBuf::from("/project/adl-no-strings"),
            PathBuf::from("/project/adl-strings"),
        ];
        let source_uri = Url::parse("file:///project/adl/common/main.adl").unwrap();
        let resolved = resolve_import(
            &package_roots,
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

    #[test]
    fn test_extract_module_name_from_content() {
        let content = r#"
module a.b.c;

import x.y.z;

struct Test {};
"#;
        assert_eq!(extract_module_name_from_content(content), Some("a.b.c".to_string()));
    }

    #[test]
    fn test_extract_module_name_with_semicolon() {
        let content = "module some.deeply.nested.module;";
        assert_eq!(extract_module_name_from_content(content), Some("some.deeply.nested.module".to_string()));
    }

    #[test]
    fn test_extract_module_name_without_semicolon() {
        let content = "module simple.module";
        assert_eq!(extract_module_name_from_content(content), Some("simple.module".to_string()));
    }

    #[test]
    fn test_extract_module_name_with_extra_whitespace() {
        let content = "   module   whitespace.test   ;   ";
        assert_eq!(extract_module_name_from_content(content), Some("whitespace.test".to_string()));
    }

    #[test]
    fn test_discover_package_root_from_file() {
        let file_uri = Url::parse("file:///project/adl/a/b/c.adl").unwrap();
        let module_name = "a.b.c";
        let package_root = discover_package_root_from_file(&file_uri, module_name);
        assert_eq!(package_root, Some(PathBuf::from("/project/adl")));
    }

    #[test]
    fn test_discover_package_root_single_module() {
        let file_uri = Url::parse("file:///project/adl/simple.adl").unwrap();
        let module_name = "simple";
        let package_root = discover_package_root_from_file(&file_uri, module_name);
        assert_eq!(package_root, Some(PathBuf::from("/project/adl")));
    }

    #[test]
    fn test_load_package_definition() {
        use serde_json::json;
        use std::io::Write;
        
        // Create a temporary directory and file for testing
        let temp_dir = std::env::temp_dir().join("adl_lsp_test");
        std::fs::create_dir_all(&temp_dir).unwrap();
        
        let package_file = temp_dir.join("adl-package.json");
        let package_content = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": [
                {
                    "name": "common-types",
                    "path": "../common"
                },
                {
                    "name": "utils",
                    "path": "./lib/utils"
                }
            ]
        });
        
        let mut file = std::fs::File::create(&package_file).unwrap();
        file.write_all(package_content.to_string().as_bytes()).unwrap();
        
        let package_def = load_package_definition(&package_file).unwrap();
        assert_eq!(package_def.name, "test-package");
        assert_eq!(package_def.version, "1.0.0");
        assert_eq!(package_def.dependencies.len(), 2);
        assert_eq!(package_def.dependencies[0].name, "common-types");
        assert_eq!(package_def.dependencies[0].path, "../common");
        assert_eq!(package_def.dependencies[1].name, "utils");
        assert_eq!(package_def.dependencies[1].path, "./lib/utils");
        
        // Clean up
        std::fs::remove_file(&package_file).unwrap();
        std::fs::remove_dir(&temp_dir).unwrap();
    }

    #[test]
    fn test_load_minimal_package_definition() {
        use serde_json::json;
        use std::io::Write;
        
        let temp_dir = std::env::temp_dir().join("adl_lsp_test_minimal");
        std::fs::create_dir_all(&temp_dir).unwrap();
        
        let package_file = temp_dir.join("adl-package.json");
        let package_content = json!({
            "name": "minimal-package"
        });
        
        let mut file = std::fs::File::create(&package_file).unwrap();
        file.write_all(package_content.to_string().as_bytes()).unwrap();
        
        let package_def = load_package_definition(&package_file).unwrap();
        assert_eq!(package_def.name, "minimal-package");
        assert_eq!(package_def.version, ""); // default value
        assert_eq!(package_def.dependencies.len(), 0); // default value
        
        // Clean up
        std::fs::remove_file(&package_file).unwrap();
        std::fs::remove_dir(&temp_dir).unwrap();
    }
}
