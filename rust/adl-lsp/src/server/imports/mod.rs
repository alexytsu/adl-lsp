use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{Arc, RwLock};

use async_lsp::lsp_types::Url;
use tracing::{debug, trace};

use crate::node::{AdlImportDeclaration, AdlModuleDefinition, NodeKind};
use crate::parser::ParsedTree;
use crate::parser::tree::Tree;
use crate::server::modules;

mod fqn;
pub use fqn::Fqn;

/// A table that caches resolved imports to avoid repeated tree traversals
#[derive(Debug, Clone, Default)]
pub struct ImportsCache {
    /// Maps FQN -> target_uri where the symbol is defined
    /// Since ADL workspaces cannot have duplicate symbol definitions,
    /// each identifier maps to exactly one location
    definition_locations: Arc<RwLock<HashMap<Fqn, Url>>>,

    /// Maps source_uri -> set of all FQNs it imports
    /// Used for efficient invalidation when a document changes
    imported_symbols: Arc<RwLock<HashMap<Url, HashSet<Fqn>>>>,

    /// Maps source_uri -> set of all FQNs it defines
    /// Used for efficient invalidation when a document changes
    defined_symbols: Arc<RwLock<HashMap<Url, HashSet<Fqn>>>>,
}

impl ImportsCache {
    pub fn clear(&mut self) {
        self.definition_locations.write().expect("poisoned").clear();
        self.imported_symbols.write().expect("poisoned").clear();
        self.defined_symbols.write().expect("poisoned").clear();
    }

    /// Attempt to lookup the uri where an identifier is defined
    pub fn lookup_fqn(&self, fqn: &Fqn) -> Option<Url> {
        self.definition_locations
            .read()
            .expect("poisoned")
            .get(fqn)
            .cloned()
    }

    /// Get all files that import a specific type
    pub fn get_files_importing_type(&self, fqn: &Fqn) -> Vec<Url> {
        let imported_symbols = self.imported_symbols.read().expect("poisoned");
        imported_symbols
            .iter()
            .filter_map(|(uri, symbols)| {
                if symbols.contains(fqn) {
                    Some(uri.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Add a validated import to the table, registering its import and definition
    fn add_import(&self, source_uri: &Url, fqn: &Fqn, target_uri: &Url) {
        let mut definition_locations = self.definition_locations.write().expect("poisoned");
        let mut imported_symbols = self.imported_symbols.write().expect("poisoned");
        let mut defined_symbols = self.defined_symbols.write().expect("poisoned");

        definition_locations.insert(fqn.clone(), target_uri.clone());
        imported_symbols
            .entry(source_uri.clone())
            .or_default()
            .insert(fqn.clone());
        defined_symbols
            .entry(target_uri.clone())
            .or_default()
            .insert(fqn.clone());
    }

    /// Add a definition to the table (for symbols defined in a file)
    fn add_definition(&self, source_uri: &Url, fqn: &Fqn) {
        let mut definition_locations = self.definition_locations.write().expect("poisoned");
        let mut defined_symbols = self.defined_symbols.write().expect("poisoned");

        definition_locations.insert(fqn.clone(), source_uri.clone());
        defined_symbols
            .entry(source_uri.clone())
            .or_default()
            .insert(fqn.clone());
    }

    /// Clear all imports and definitions for a given source URI that has been updated
    fn clear_source_caches(&self, source_uri: &Url) {
        let mut imported_symbols = self.imported_symbols.write().expect("poisoned");
        let mut definition_table = self.defined_symbols.write().expect("poisoned");
        let mut definition_locations = self.definition_locations.write().expect("poisoned");

        trace!(
            "clearing imports and definitions for source file: {}",
            source_uri.path()
        );

        // Remove the import cache for this source
        imported_symbols.remove(source_uri).unwrap_or_default();

        // Remove the definition cache for this source
        let definitions = { definition_table.remove(source_uri).unwrap_or_default() };

        // Remove the definition locations for all symbols in this source
        for identifier in definitions {
            definition_locations.remove(&identifier);
        }
    }
}

/// Import management trait that handles import resolution and caching
pub trait ImportManager {
    /// Get the imports cache
    fn cache(&self) -> &ImportsCache;

    /// Clear the imports cache
    fn clear_cache(&mut self);

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

    /// Clear the imports cache
    fn clear_cache(&mut self) {
        self.clear();
    }

    /// Resolve imports from a document and populate the imports table
    fn resolve_document_imports(
        &self,
        package_roots: &[std::path::PathBuf],
        source_uri: &Url,
        source_tree: &ParsedTree,
        source_content: &[u8],
        get_or_parse_document_tree: &mut impl FnMut(&Url) -> Option<ParsedTree>,
    ) {
        trace!("resolving imports for document: {}", source_uri);

        // Clear existing imports for this source
        self.clear_source_caches(source_uri);

        // Register all type definitions in this document
        self.register_document_definitions(source_uri, source_tree, source_content);

        // Find all import declarations in the document
        let import_nodes = source_tree.find_all_nodes(NodeKind::is_import_declaration);

        let module_definition = source_tree
            .find_first_node(NodeKind::is_module_definition)
            .expect("expected module definition");
        let module_definition =
            AdlModuleDefinition::try_new(module_definition).expect("expected module definition");

        for import_node in import_nodes {
            let import_node =
                AdlImportDeclaration::try_new(import_node).expect("expected import_declaration");
            self.process_import_declaration(
                package_roots,
                source_uri,
                module_definition.module_name(source_content),
                source_content,
                &import_node,
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
        source_module: &str,
        source_content: &[u8],
        import_node: &AdlImportDeclaration,
        get_or_parse_document_tree: &mut impl FnMut(&Url) -> Option<ParsedTree>,
    ) {
        match import_node {
            AdlImportDeclaration::FullyQualified(_) => {
                // Extract imported FQN from the import declaration
                let fqn = Fqn::from_module_name_and_type_name(
                    import_node.module_name(source_content),
                    import_node
                        .imported_type_name(source_content)
                        .expect(" expected FullyQualified import to have a type_name "),
                );
                self.resolve_fully_qualified_import(package_roots, source_uri, source_module, &fqn);
            }
            AdlImportDeclaration::StarImport(_) => {
                let imported_module_path =
                    import_node.module_name(source_content).split('.').collect();
                // let imported_module_tree =
                //     &ParsedTree::get_source_module(import_node.inner(), source_content)
                //         .unwrap_or_default();
                self.expand_star_import(
                    package_roots,
                    source_uri,
                    source_module,
                    &imported_module_path,
                    get_or_parse_document_tree,
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
        imported_module_path: &Vec<&str>,
        get_or_parse_document_tree: &mut impl FnMut(&Url) -> Option<ParsedTree>,
    ) {
        debug!("expanding star import from {:?}", imported_module_path);

        let imported_module = modules::resolve_import(
            package_roots,
            source_uri,
            source_module,
            imported_module_path,
            &|path| fs::exists(path).is_ok_and(|exists| exists),
        );

        if let Some(ref target_uri) = imported_module {
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
    fn find_type_definitions(&self, tree: &ParsedTree, content: &[u8]) -> Vec<Fqn> {
        let mut type_names = Vec::new();

        let module_definition = tree
            .find_first_node(NodeKind::is_module_definition)
            .expect("expected module definition");
        let module_definition =
            AdlModuleDefinition::try_new(module_definition).expect("expected module definition");
        let module_name = module_definition.module_name(content);

        // Find all types defined locally in this module
        let type_def_nodes = tree.find_all_nodes(NodeKind::is_local_definition);

        for type_def in type_def_nodes {
            // TODO: create an AdlNode here to handle the type name logic
            // The type name is the first child that is a type_name
            if let Some(type_name_node) = type_def
                .children(&mut type_def.walk())
                .find(|child| NodeKind::is_type_name(child))
            {
                if let Ok(type_name) = type_name_node.utf8_text(content) {
                    type_names.push(Fqn::from_module_name_and_type_name(module_name, type_name));
                }
            }
        }

        debug!("found type definitions: {:?}", type_names);
        type_names
    }

    /// Resolve a fully-qualified import
    fn resolve_fully_qualified_import(
        &self,
        package_roots: &[std::path::PathBuf],
        source_uri: &Url,
        source_module: &str,
        import: &Fqn,
    ) {
        debug!("resolving fully-qualified import: {:?}", import);

        // Resolve the module paths
        let possible_paths = modules::resolve_import(
            package_roots,
            source_uri,
            source_module,
            &import.module_path(),
            &|path| fs::exists(path).is_ok_and(|exists| exists),
        );

        if let Some(ref target_uri) = possible_paths {
            // Only add the symbol if the target file actually exists
            if std::fs::metadata(target_uri.path()).is_ok() {
                trace!(
                    "target file exists, adding to imports table: {}",
                    target_uri.path()
                );
                // TODO: check if the target file actually contains the definition
                self.add_import(source_uri, import, target_uri);
            } else {
                trace!(
                    "target file does not exist, skipping: {}",
                    target_uri.path()
                );
            }
        }
    }
}
