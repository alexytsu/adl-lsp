#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fqn {
    module_name: String,
    type_name: String,
}

impl Fqn {
    pub fn from_module_name_and_type_name(module_name: &str, type_name: &str) -> Self {
        Self {
            module_name: module_name.to_string(),
            type_name: type_name.to_string(),
        }
    }

    /// Returns the module path parts as a vector of strings
    /// e.g. "adlc.package" -> ["adlc", "package"]
    ///
    /// # Example
    ///
    /// ```
    /// use adl_lsp::server::imports::Fqn;
    ///
    /// let fqn = Fqn::from_module_name_and_type_name("adlc.package", "AdlPackage");
    /// assert_eq!(fqn.module_path_parts(), vec!["adlc", "package"]);   
    /// ```
    pub fn module_path_parts(&self) -> Vec<&str> {
        self.module_name.split(".").collect()
    }

    /// Get the module name
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    /// Get the type name
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// Get the full qualified name as a string
    /// e.g. "adlc.package.AdlPackage"
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.module_name, self.type_name)
    }

    /// Check if this FQN's module matches the given prefix parts
    /// Used for incremental completion suggestions
    pub fn module_matches_prefix(&self, prefix_parts: &[&str]) -> bool {
        let module_parts = self.module_path_parts();
        if prefix_parts.len() > module_parts.len() {
            return false;
        }
        
        module_parts.iter()
            .zip(prefix_parts.iter())
            .all(|(module_part, prefix_part)| module_part == prefix_part)
    }

    /// Get the next module part after the given prefix
    /// Returns None if the prefix doesn't match or if there's no next part
    pub fn next_module_part_after_prefix(&self, prefix_parts: &[&str]) -> Option<&str> {
        let module_parts = self.module_path_parts();
        if !self.module_matches_prefix(prefix_parts) {
            return None;
        }
        
        module_parts.get(prefix_parts.len()).copied()
    }
}
