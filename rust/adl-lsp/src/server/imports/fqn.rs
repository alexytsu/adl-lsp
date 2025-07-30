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
}
