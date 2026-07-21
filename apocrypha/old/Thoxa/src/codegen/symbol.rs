//! Symbol table for tracking variables and their QBE names

use std::collections::HashMap;
use crate::codegen::types::TypeId;
use crate::error::{Error, Result};

/// Information about a variable
#[derive(Debug, Clone)]
pub struct Symbol {
    /// The variable's name in the source
    pub name: String,
    /// The QBE name (e.g., %x.1)
    pub qbe_name: String,
    /// The type of this variable
    pub type_id: TypeId,
    /// Whether this is mutable
    pub is_mut: bool,
    /// Whether this variable needs reference counting
    pub needs_refcount: bool,
}

/// A scope in the symbol table
#[derive(Debug)]
struct Scope {
    /// Symbols in this scope
    symbols: HashMap<String, Symbol>,
}

/// Symbol table with lexical scoping
pub struct SymbolTable {
    /// Stack of scopes (innermost last)
    scopes: Vec<Scope>,
    /// Counter for generating unique names
    next_id: u32,
}

impl SymbolTable {
    /// Create a new symbol table with global scope
    pub fn new() -> Self {
        SymbolTable {
            scopes: vec![Scope { symbols: HashMap::new() }],
            next_id: 0,
        }
    }
    
    /// Enter a new scope
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope { symbols: HashMap::new() });
    }
    
    /// Exit the current scope and return variables that need cleanup
    pub fn pop_scope(&mut self) -> Result<Vec<Symbol>> {
        if self.scopes.len() <= 1 {
            return Err(Error::Codegen("Cannot pop global scope".to_string()));
        }
        
        let scope = self.scopes.pop().unwrap();
        
        // Collect all variables that need reference counting
        let mut cleanup = Vec::new();
        for symbol in scope.symbols.values() {
            if symbol.needs_refcount {
                cleanup.push(symbol.clone());
            }
        }
        
        Ok(cleanup)
    }
    
    /// Enter a new scope (alias for push_scope)
    pub fn enter_scope(&mut self) {
        self.push_scope();
    }
    
    /// Exit the current scope (alias for pop_scope) and return cleanup list
    pub fn exit_scope(&mut self) -> Vec<Symbol> {
        // Ignore errors for now (can't pop global scope)
        self.pop_scope().unwrap_or_else(|_| Vec::new())
    }
    
    /// Get all variables that need cleanup from all scopes (for return statement)
    pub fn get_all_cleanup_vars(&self) -> Vec<Symbol> {
        let mut cleanup = Vec::new();
        for scope in &self.scopes {
            for symbol in scope.symbols.values() {
                if symbol.needs_refcount {
                    cleanup.push(symbol.clone());
                }
            }
        }
        cleanup
    }
    
    /// Add a variable to the current scope
    pub fn insert(&mut self, name: String, type_id: TypeId, is_mut: bool) -> Result<Symbol> {
        // Default behavior: assume pointers need refcounting
        // This is for backwards compatibility with existing code
        let needs_refcount = type_id == TypeId::POINTER;
        self.insert_with_refcount(name, type_id, is_mut, needs_refcount)
    }
    
    /// Add a variable to the current scope with explicit refcount flag
    pub fn insert_with_refcount(&mut self, name: String, type_id: TypeId, is_mut: bool, needs_refcount: bool) -> Result<Symbol> {
        let current_scope = self.scopes.last_mut()
            .ok_or_else(|| Error::Codegen("No active scope".to_string()))?;
        
        // Check for duplicate in current scope
        if current_scope.symbols.contains_key(&name) {
            return Err(Error::Codegen(format!("Variable '{}' already defined in this scope", name)));
        }
        
        // Generate unique QBE name
        let qbe_name = format!("%{}.{}", name, self.next_id);
        self.next_id += 1;
        
        let symbol = Symbol {
            name: name.clone(),
            qbe_name,
            type_id,
            is_mut,
            needs_refcount,
        };
        
        current_scope.symbols.insert(name, symbol.clone());
        Ok(symbol)
    }
    
    /// Add a borrowed variable to the current scope (doesn't need cleanup)
    pub fn insert_borrowed(&mut self, name: String, type_id: TypeId) -> Result<Symbol> {
        let current_scope = self.scopes.last_mut()
            .ok_or_else(|| Error::Codegen("No active scope".to_string()))?;
        
        // Check for duplicate in current scope
        if current_scope.symbols.contains_key(&name) {
            return Err(Error::Codegen(format!("Variable '{}' already defined in this scope", name)));
        }
        
        // Generate unique QBE name
        let qbe_name = format!("%{}.{}", name, self.next_id);
        self.next_id += 1;
        
        // Borrowed references don't need reference counting
        let needs_refcount = false;
        
        let symbol = Symbol {
            name: name.clone(),
            qbe_name,
            type_id,
            is_mut: false,  // Borrowed references are immutable
            needs_refcount,
        };
        
        current_scope.symbols.insert(name, symbol.clone());
        Ok(symbol)
    }
    
    /// Add a function parameter to the current scope (uses parameter name directly)
    pub fn insert_param(&mut self, name: String, type_id: TypeId, is_mut: bool, needs_refcount: bool) -> Result<Symbol> {
        let current_scope = self.scopes.last_mut()
            .ok_or_else(|| Error::Codegen("No active scope".to_string()))?;
        
        // Check for duplicate in current scope
        if current_scope.symbols.contains_key(&name) {
            return Err(Error::Codegen(format!("Parameter '{}' already defined", name)));
        }
        
        // For parameters, use the name directly with % prefix
        let qbe_name = format!("%{}", name);
        
        let symbol = Symbol {
            name: name.clone(),
            qbe_name,
            type_id,
            is_mut,
            needs_refcount,
        };
        
        current_scope.symbols.insert(name, symbol.clone());
        Ok(symbol)
    }
    
    /// Look up a variable (searches all scopes from innermost to outermost)
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        // Search from innermost scope outward
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.symbols.get(name) {
                return Some(symbol);
            }
        }
        None
    }
    
    /// Generate a unique temporary name
    pub fn gen_temp(&mut self) -> String {
        let name = format!("%t.{}", self.next_id);
        self.next_id += 1;
        name
    }
    
    /// Get the current scope depth (for debugging)
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_operations() {
        let mut table = SymbolTable::new();
        let type_id = TypeId(0); // Dummy type ID
        
        // Insert a variable
        let x = table.insert("x".to_string(), type_id, false).unwrap();
        assert_eq!(x.name, "x");
        assert!(x.qbe_name.starts_with("%x."));
        assert!(!x.is_mut);
        
        // Look it up
        let found = table.lookup("x").unwrap();
        assert_eq!(found.name, "x");
        
        // Non-existent variable
        assert!(table.lookup("y").is_none());
    }
    
    #[test]
    fn test_scoping() {
        let mut table = SymbolTable::new();
        let type_id = TypeId(0);
        
        // Global scope
        table.insert("x".to_string(), type_id, false).unwrap();
        
        // Enter new scope
        table.push_scope();
        
        // Shadow x in inner scope
        table.insert("x".to_string(), type_id, true).unwrap();
        
        // Inner x is mutable
        let inner_x = table.lookup("x").unwrap();
        assert!(inner_x.is_mut);
        
        // Exit inner scope
        table.pop_scope().unwrap();
        
        // Back to immutable x
        let outer_x = table.lookup("x").unwrap();
        assert!(!outer_x.is_mut);
    }
    
    #[test]
    fn test_duplicate_in_same_scope() {
        let mut table = SymbolTable::new();
        let type_id = TypeId(0);
        
        // First insert succeeds
        table.insert("x".to_string(), type_id, false).unwrap();
        
        // Second insert in same scope fails
        let result = table.insert("x".to_string(), type_id, false);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_temp_generation() {
        let mut table = SymbolTable::new();
        
        let t1 = table.gen_temp();
        let t2 = table.gen_temp();
        let t3 = table.gen_temp();
        
        assert!(t1.starts_with("%t."));
        assert!(t2.starts_with("%t."));
        assert!(t3.starts_with("%t."));
        assert_ne!(t1, t2);
        assert_ne!(t2, t3);
    }
}