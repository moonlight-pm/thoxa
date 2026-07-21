use std::collections::HashMap;

/// Function environment for tracking compiled functions
#[derive(Debug, Clone)]
pub struct FuncEnv {
    /// Maps function name to cranelift FuncId
    pub functions: HashMap<String, cranelift_module::FuncId>,
    /// Maps function name to variadic info (is_variadic, fixed_param_count)
    pub variadic_info: HashMap<String, (bool, usize)>,
    /// Current module context (for resolving unqualified function names)
    pub current_module: Option<String>,
}

impl FuncEnv {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            variadic_info: HashMap::new(),
            current_module: None,
        }
    }
    
    pub fn with_module(module: String) -> Self {
        Self {
            functions: HashMap::new(),
            variadic_info: HashMap::new(),
            current_module: Some(module),
        }
    }
    
    pub fn set_current_module(&mut self, module: Option<String>) {
        self.current_module = module;
    }
    
    pub fn declare_function(&mut self, name: String, func_id: cranelift_module::FuncId) {
        self.functions.insert(name, func_id);
    }
    
    pub fn declare_variadic_function(&mut self, name: String, func_id: cranelift_module::FuncId, is_variadic: bool, fixed_param_count: usize) {
        self.functions.insert(name.clone(), func_id);
        self.variadic_info.insert(name, (is_variadic, fixed_param_count));
    }
    
    pub fn get_variadic_info(&self, name: &str) -> Option<(bool, usize)> {
        // First try the name as given
        if let Some(info) = self.variadic_info.get(name) {
            return Some(*info);
        }
        
        // If not found and we're in a module context, try with module prefix
        if let Some(ref module) = self.current_module {
            let qualified_name = format!("{}.{}", module, name);
            self.variadic_info.get(&qualified_name).copied()
        } else {
            None
        }
    }
    
    pub fn get_function(&self, name: &str) -> Option<cranelift_module::FuncId> {
        // First try the name as given
        if let Some(func_id) = self.functions.get(name).copied() {
            return Some(func_id);
        }
        
        // If not found and we're in a module context, try with module prefix
        if let Some(ref module) = self.current_module {
            let qualified_name = format!("{}.{}", module, name);
            self.functions.get(&qualified_name).copied()
        } else {
            None
        }
    }
}