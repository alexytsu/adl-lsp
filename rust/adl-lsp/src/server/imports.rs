use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use async_lsp::lsp_types::Url;
use tracing::{debug, trace};

use crate::node::NodeKind;
use crate::parser::ParsedTree;
use crate::parser::definition::UnresolvedImport;
use crate::parser::tree::Tree;
use crate::server::modules;

/// A table that caches resolved imports to avoid repeated tree traversals
#[derive(Debug, Clone, Default)]
pub struct ImportsCache {
    /// Maps identifier -> target_uri where the symbol is defined
    /// Since ADL workspaces cannot have duplicate symbol definitions,
    /// each identifier maps to exactly one location
    definition_locations: Arc<RwLock<HashMap<String, Url>>>,

    /// Maps source_uri -> set of all identifiers it imports
    /// Used for efficient invalidation when a document changes
    imported_symbols: Arc<RwLock<HashMap<Url, HashSet<String>>>>,

    /// Maps source_uri -> set of all identifiers it defines
    /// Used for efficient invalidation when a document changes
    defined_symbols: Arc<RwLock<HashMap<Url, HashSet<String>>>>,
}

impl ImportsCache {
    /// Attempt to lookup the uri where an identifier is defined
    pub fn lookup_identifier(&self, identifier: &str) -> Option<Url> {
        self.definition_locations
            .read()
            .expect("poisoned")
            .get(identifier)
            .cloned()
    }

    /// Get all files that import a specific identifier
    pub fn get_files_importing_identifier(&self, identifier: &str) -> Vec<Url> {
        let imported_symbols = self.imported_symbols.read().expect("poisoned");
        imported_symbols
            .iter()
            .filter_map(|(uri, symbols)| {
                if symbols.contains(identifier) {
                    Some(uri.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Add a validated import to the table, registering its import and definition
    fn add_import(&self, source_uri: &Url, identifier: &String, target_uri: &Url) {
        {
            let mut definition_locations = self.definition_locations.write().expect("poisoned");
            definition_locations.insert(identifier.into(), target_uri.clone());
        }

        {
            let mut imported_symbols = self.imported_symbols.write().expect("poisoned");
            imported_symbols
                .entry(source_uri.clone())
                .or_default()
                .insert(identifier.into());
        }

        {
            let mut defined_symbols = self.defined_symbols.write().expect("poisoned");
            defined_symbols
                .entry(target_uri.clone())
                .or_default()
                .insert(identifier.into());
        }
    }

    /// Add a definition to the table (for symbols defined in a file)
    fn add_definition(&self, source_uri: &Url, identifier: &str) {
        {
            let mut definition_locations = self.definition_locations.write().expect("poisoned");
            definition_locations.insert(identifier.to_string(), source_uri.clone());
        }

        {
            let mut defined_symbols = self.defined_symbols.write().expect("poisoned");
            defined_symbols
                .entry(source_uri.clone())
                .or_default()
                .insert(identifier.to_string());
        }
    }

    /// Clear all imports and definitions for a given source URI that has been updated
    fn clear_source_caches(&self, source_uri: &Url) {
        trace!(
            "clearing imports and definitions for source file: {}",
            source_uri.path()
        );

        // Remove the import cache for this source
        {
            let mut imported_symbols = self.imported_symbols.write().expect("poisoned");
            imported_symbols.remove(source_uri).unwrap_or_default();
        }

        // Remove the definition cache for this source
        let definitions = {
            let mut definitions = self.defined_symbols.write().expect("poisoned");
            definitions.remove(source_uri).unwrap_or_default()
        };

        // Remove the definition locations for all symbols in this source
        {
            let mut definition_locations = self.definition_locations.write().expect("poisoned");
            for identifier in definitions {
                definition_locations.remove(&identifier);
            }
        }
    }
}

/// Import management trait that handles import resolution and caching
pub trait ImportManager {
    /// Get the imports cache
    fn cache(&self) -> &ImportsCache;

    /// Resolve imports from a document and populate the imports table
    fn resolve_document_imports(
        &self,
        package_roots: &[std::path::PathBuf],
        source_uri: &Url,
        tree: &ParsedTree,
        content: &[u8],
        get_or_parse_document_tree: &mut impl FnMut(&Url) -> Option<ParsedTree>,
    );
}

impl ImportManager for ImportsCache {
    /// Get the imports cache
    fn cache(&self) -> &ImportsCache {
        self
    }

    /// Resolve imports from a document and populate the imports table
    fn resolve_document_imports(
        &self,
        package_roots: &[std::path::PathBuf],
        source_uri: &Url,
        tree: &ParsedTree,
        content: &[u8],
        get_or_parse_document_tree: &mut impl FnMut(&Url) -> Option<ParsedTree>,
    ) {
        trace!("resolving imports for document: {}", source_uri);

        // Clear existing imports for this source
        self.clear_source_caches(source_uri);

        // Register all type definitions in this document
        self.register_document_definitions(source_uri, tree, content);

        // Find all import declarations in the document
        let import_nodes = tree.find_all_nodes(NodeKind::is_import_declaration);

        for import_node in import_nodes {
            self.process_import_declaration(
                package_roots,
                source_uri,
                &import_node,
                content,
                get_or_parse_document_tree,
            );
        }
    }
}

impl ImportsCache {
    /// Register all type definitions in a document
    fn register_document_definitions(&self, source_uri: &Url, tree: &ParsedTree, content: &[u8]) {
        debug!("registering definitions for document: {}", source_uri);

        // Find all type definitions in this document
        let type_definitions = self.find_type_definitions(tree, content);

        // Register each definition in the cache
        for definition in type_definitions {
            self.add_definition(source_uri, &definition);
        }
    }

    /// Process a single import declaration node
    fn process_import_declaration(
        &self,
        package_roots: &[std::path::PathBuf],
        source_uri: &Url,
        import_node: &tree_sitter::Node,
        content: &[u8],
        get_or_parse_document_tree: &mut impl FnMut(&Url) -> Option<ParsedTree>,
    ) {
        // Extract import path from the import declaration
        if let Some(import_path_node) = import_node.child(1) {
            let import_path_text = import_path_node.utf8_text(content).unwrap_or("");

            // Parse the import path
            let parts: Vec<&str> = import_path_text.split('.').collect();
            if parts.is_empty() {
                return;
            }

            let source_module =
                &ParsedTree::get_source_module(import_node, content).unwrap_or_default();

            // Check if this is a star import (ends with *)
            if parts.last() == Some(&"*") {
                // Star import - expand by visiting the target module
                let target_module_path: Vec<String> = parts[..parts.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                self.expand_star_import(
                    package_roots,
                    source_uri,
                    source_module,
                    &target_module_path,
                    get_or_parse_document_tree,
                );
            } else {
                // Regular import - just the last identifier
                let identifier = parts.last().unwrap().to_string();
                let target_module_path: Vec<String> = parts[..parts.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                self.resolve_regular_import(
                    package_roots,
                    source_uri,
                    source_module,
                    &target_module_path,
                    &identifier,
                );
            }
        }
    }

    /// Expand a star import by finding all type definitions in the target module
    fn expand_star_import(
        &self,
        package_roots: &[std::path::PathBuf],
        source_uri: &Url,
        source_module: &str,
        target_module_path: &Vec<String>,
        get_or_parse_document_tree: &mut impl FnMut(&Url) -> Option<ParsedTree>,
    ) {
        debug!("expanding star import from {:?}", target_module_path);

        // Create an unresolved import to find the target module
        let unresolved = UnresolvedImport {
            source_module: source_module.into(),
            target_module_path: target_module_path.clone(),
            identifier: "*".to_string(),
        };

        // Resolve the module paths
        let possible_paths = modules::resolve_import(package_roots, source_uri, &unresolved);

        for ref target_uri in possible_paths {
            // Try to read the target module
            if let Ok(target_content) = std::fs::read_to_string(target_uri.path()) {
                // Get or parse the target tree
                if let Some(target_tree) = get_or_parse_document_tree(target_uri) {
                    // Find all type definitions in the target module
                    let type_definitions =
                        self.find_type_definitions(&target_tree, target_content.as_bytes());

                    // Add each type definition as an imported symbol
                    for ref type_name in type_definitions {
                        self.add_import(source_uri, type_name, target_uri);
                    }
                }
            }
        }
    }

    /// Find all type definition names in a parsed tree
    fn find_type_definitions(&self, tree: &ParsedTree, content: &[u8]) -> Vec<String> {
        let mut type_names = Vec::new();

        // Find all type definition nodes
        let type_def_nodes = tree.find_all_nodes(|node| {
            NodeKind::is_type_definition(node)
                || NodeKind::is_struct_definition(node)
                || NodeKind::is_union_definition(node)
                || NodeKind::is_newtype_definition(node)
        });

        for node in type_def_nodes {
            // The type name is typically the first child that is a type_name
            if let Some(type_name_node) = node
                .children(&mut node.walk())
                .find(|child| NodeKind::is_type_name(child))
            {
                if let Ok(type_name) = type_name_node.utf8_text(content) {
                    type_names.push(type_name.to_string());
                }
            }
        }

        debug!("found type definitions: {:?}", type_names);
        type_names
    }

    /// Resolve a regular (non-star) import
    fn resolve_regular_import(
        &self,
        package_roots: &[std::path::PathBuf],
        source_uri: &Url,
        source_module: &str,
        target_module_path: &Vec<String>,
        identifier: &String,
    ) {
        debug!(
            "resolving regular import: {} from {:?}",
            identifier, target_module_path
        );

        // Create an unresolved import to find the target module
        let unresolved = UnresolvedImport {
            source_module: source_module.into(),
            target_module_path: target_module_path.clone(),
            identifier: identifier.clone(),
        };

        // Resolve the module paths
        let possible_paths = modules::resolve_import(package_roots, source_uri, &unresolved);

        for ref target_uri in possible_paths {
            // Only add the symbol if the target file actually exists
            if std::fs::metadata(target_uri.path()).is_ok() {
                trace!(
                    "target file exists, adding to imports table: {}",
                    target_uri.path()
                );
                // TODO: check if the target file actually contains the definition
                self.add_import(source_uri, identifier, target_uri);
            } else {
                trace!(
                    "target file does not exist, skipping: {}",
                    target_uri.path()
                );
            }
        }
    }
}
