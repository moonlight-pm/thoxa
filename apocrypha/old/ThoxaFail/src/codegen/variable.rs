use cranelift::prelude::*;
use std::collections::HashMap;

/// Variable environment for tracking let bindings during code generation
#[derive(Debug, Clone)]
pub struct VarEnv {
    variables: HashMap<String, (Variable, bool)>, // (Variable, is_mutable)
    next_var: usize,
}

impl VarEnv {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            next_var: 0,
        }
    }
    
    pub fn declare_variable(&mut self, name: String, var: Variable, is_mutable: bool) {
        self.variables.insert(name, (var, is_mutable));
    }
    
    pub fn get_variable(&self, name: &str) -> Option<Variable> {
        self.variables.get(name).map(|(var, _)| *var)
    }
    
    pub fn is_mutable(&self, name: &str) -> Option<bool> {
        self.variables.get(name).map(|(_, is_mut)| *is_mut)
    }
    
    pub fn next_variable(&mut self) -> Variable {
        let var = Variable::new(self.next_var);
        self.next_var += 1;
        var
    }
    
    pub fn set_next_var(&mut self, next: usize) {
        self.next_var = next;
    }
    
    pub fn get_all_variables(&self) -> &HashMap<String, (Variable, bool)> {
        &self.variables
    }
}