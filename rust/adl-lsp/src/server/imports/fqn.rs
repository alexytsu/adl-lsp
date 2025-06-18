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

    pub fn module_path(&self) -> Vec<&str> {
        self.module_name.split(".").collect()
    }
}
