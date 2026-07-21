use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::fs;
use crate::ast::{Expr, ImportSpec};
use crate::lexer::lex;
use crate::parser::parse;

/// Handles file-based module compilation and resolution
pub struct ModuleCompiler {
    /// Cache of compiled modules by absolute path
    pub file_cache: HashMap<PathBuf, CompiledModule>,
    /// Dependency graph: module -> set of modules it imports
    dependency_graph: HashMap<PathBuf, HashSet<PathBuf>>,
    /// Current working directory for relative imports
    _working_dir: PathBuf,
}

/// A compiled module from a file
#[derive(Debug, Clone)]
pub struct CompiledModule {
    /// The module's absolute path
    pub path: PathBuf,
    /// The parsed AST (should be a module expression)
    pub ast: Expr,
    /// Import statements found in this module
    pub imports: Vec<ImportInfo>,
}

/// Information about an import statement
#[derive(Debug, Clone)]
pub struct ImportInfo {
    /// The import specification (what's being imported)
    pub spec: ImportSpec,
    /// The import path as written in the source
    pub path: String,
    /// The resolved absolute path to the module file
    pub resolved_path: PathBuf,
}

impl ModuleCompiler {
    pub fn new() -> Self {
        Self {
            file_cache: HashMap::new(),
            dependency_graph: HashMap::new(),
            _working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
    
    /// Parse module contents, handling export keywords and imports at the top level
    fn parse_module_contents(&self, contents: &str, path: &Path) -> Result<Expr, String> {
        let module_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("Cannot determine module name from path {:?}", path))?
            .to_string();
        
        // For module files that contain imports and/or exports, we need special handling
        // We'll preprocess the content to wrap export statements inside a module definition
        // while keeping imports at the top level
        
        let has_imports = contents.contains("import ");
        let has_exports = contents.contains("export ");
        
        if has_imports || has_exports {
            // Split content by lines to process line by line
            let lines: Vec<&str> = contents.lines().collect();
            let mut import_lines = Vec::new();
            let mut module_lines = Vec::new();
            
            for line in lines {
                let trimmed = line.trim();
                
                // Skip comments
                if trimmed.starts_with("#") || trimmed.is_empty() {
                    continue;
                }
                
                // Handle multi-line constructs (simplified for now)
                if trimmed.starts_with("import ") {
                    import_lines.push(line);
                } else {
                    // Everything else goes into the module
                    module_lines.push(line);
                }
            }
            
            // Build the transformed content
            let mut transformed = String::new();
            
            // Add imports first
            for line in import_lines {
                transformed.push_str(line);
                transformed.push('\n');
            }
            
            // Then add the module with remaining content
            if !module_lines.is_empty() || !has_exports {
                transformed.push_str(&format!("type {} {{\n", module_name));
                for line in module_lines {
                    transformed.push_str(line);
                    transformed.push('\n');
                }
                transformed.push_str("}\n");
            }
            
            // Parse the transformed content
            let tokens = lex(&transformed);
            parse(tokens)
        } else {
            // No imports or exports - use the old wrapping approach
            let wrapped_content = format!("type {} {{\n{}\n}}", module_name, contents);
            let tokens = lex(&wrapped_content);
            parse(tokens)
        }
    }
    
    /// Resolve an import path relative to the current file
    pub fn resolve_import_path(&self, import_path: &str, current_file: &Path) -> Result<PathBuf, String> {
        let path = if import_path.starts_with("./") || import_path.starts_with("../") {
            // Relative import
            let base_dir = current_file.parent()
                .ok_or_else(|| format!("Cannot determine parent directory of {:?}", current_file))?;
            base_dir.join(import_path)
        } else if import_path.starts_with("thoxa/") {
            // Standard library import (future feature)
            return Err("Standard library imports not yet implemented".to_string());
        } else {
            // Local import (same directory)
            let base_dir = current_file.parent()
                .ok_or_else(|| format!("Cannot determine parent directory of {:?}", current_file))?;
            base_dir.join(import_path)
        };
        
        // Add .thx extension if not present
        let path_with_ext = if path.extension().is_none() {
            path.with_extension("thx")
        } else {
            path
        };
        
        // Canonicalize to absolute path
        path_with_ext.canonicalize()
            .map_err(|e| format!("Cannot resolve path {:?}: {}", path_with_ext, e))
    }
    
    /// Compile a single file into a module
    pub fn compile_file(&mut self, path: &Path) -> Result<CompiledModule, String> {
        // Check cache first
        if let Some(module) = self.file_cache.get(path) {
            return Ok(module.clone());
        }
        
        // Read the file
        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Cannot read file {:?}: {}", path, e))?;
        
        // For module files, we need to handle export keywords specially
        // Check if this looks like a module file (has export statements)
        let has_exports = contents.contains("export ");
        
        let ast = if has_exports {
            // Parse as a module file with custom handling
            self.parse_module_contents(&contents, path)?
        } else {
            // Parse normally
            let tokens = lex(&contents);
            parse(tokens)?
        };
        
        // Extract imports from the AST
        let imports = self.extract_imports(&ast, path)?;
        
        // Check if this is a module file or an entry point
        // Module files should only contain module-level declarations
        let is_module_file = self.is_module_file(&ast);
        
        let module_ast = if is_module_file {
            // For module files, ensure we have a proper module structure
            // But don't try to convert sequences that already contain a module
            match &ast {
                Expr::Sequence(exprs) => {
                    // Check if any expression is already a module
                    let has_module = exprs.iter().any(|e| matches!(e, Expr::TypeDef { .. }));
                    if has_module {
                        // Already properly structured (e.g., imports followed by module)
                        ast
                    } else {
                        // Need to convert to module
                        self.ensure_module(ast, path)?
                    }
                }
                _ => self.ensure_module(ast, path)?
            }
        } else {
            // Entry point file - just return as is
            ast
        };
        
        let compiled = CompiledModule {
            path: path.to_path_buf(),
            ast: module_ast,
            imports,
        };
        
        // Cache the result
        self.file_cache.insert(path.to_path_buf(), compiled.clone());
        
        Ok(compiled)
    }
    
    /// Check if a file contains only module-level declarations
    fn is_module_file(&self, ast: &Expr) -> bool {
        match ast {
            Expr::TypeDef { .. } => true,
            Expr::Sequence(exprs) => {
                // Check if all top-level expressions are module-appropriate
                exprs.iter().all(|expr| match expr {
                    Expr::Import { .. } => true,
                    Expr::LetBinding { .. } => true,
                    Expr::TypeDef { .. } => true,
                    _ => false,
                })
            }
            Expr::LetBinding { .. } => true,
            _ => false,
        }
    }
    
    /// Extract all import statements from an AST
    fn extract_imports(&self, expr: &Expr, current_file: &Path) -> Result<Vec<ImportInfo>, String> {
        let mut imports = Vec::new();
        self.extract_imports_recursive(expr, current_file, &mut imports)?;
        Ok(imports)
    }
    
    fn extract_imports_recursive(
        &self,
        expr: &Expr,
        current_file: &Path,
        imports: &mut Vec<ImportInfo>,
    ) -> Result<(), String> {
        match expr {
            Expr::Import { imports: spec, path } => {
                let resolved_path = self.resolve_import_path(path, current_file)?;
                imports.push(ImportInfo {
                    spec: spec.clone(),
                    path: path.clone(),
                    resolved_path,
                });
            }
            Expr::Sequence(exprs) | Expr::Block(exprs) => {
                for expr in exprs {
                    self.extract_imports_recursive(expr, current_file, imports)?;
                }
            }
            Expr::TypeDef { members, .. } => {
                // Don't look inside module definitions for imports
                // (imports inside modules are scoped to that module)
                let _ = members;
            }
            // For other expressions, recursively check children
            Expr::Binary { left, right, .. } => {
                self.extract_imports_recursive(left, current_file, imports)?;
                self.extract_imports_recursive(right, current_file, imports)?;
            }
            Expr::LetBinding { value, .. } => {
                self.extract_imports_recursive(value, current_file, imports)?;
            }
            Expr::If { condition, then_expr, else_expr } => {
                self.extract_imports_recursive(condition, current_file, imports)?;
                self.extract_imports_recursive(then_expr, current_file, imports)?;
                if let Some(else_expr) = else_expr {
                    self.extract_imports_recursive(else_expr, current_file, imports)?;
                }
            }
            Expr::Loop { body } => {
                self.extract_imports_recursive(body, current_file, imports)?;
            }
            Expr::Assignment { value, .. } => {
                self.extract_imports_recursive(value, current_file, imports)?;
            }
            // Terminal expressions don't contain imports
            _ => {}
        }
        Ok(())
    }
    
    /// Ensure that a file's AST evaluates to a module
    fn ensure_module(&self, ast: Expr, file_path: &Path) -> Result<Expr, String> {
        // Derive module name from file name
        let module_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("Cannot determine module name from path {:?}", file_path))?
            .to_string();
        
        match ast {
            // Already a module - just ensure it has the right name
            Expr::TypeDef { name: _, members } => {
                Ok(Expr::TypeDef {
                    name: module_name,
                    members,
                })
            }
            
            // File contains a sequence of statements - wrap in implicit module
            Expr::Sequence(exprs) => {
                self.convert_to_module(exprs, module_name)
            }
            
            // Single expression - wrap in sequence then convert
            expr => {
                self.convert_to_module(vec![expr], module_name)
            }
        }
    }
    
    /// Convert a sequence of top-level expressions into a module
    fn convert_to_module(&self, exprs: Vec<Expr>, module_name: String) -> Result<Expr, String> {
        let mut members = Vec::new();
        let mut imports = Vec::new();
        
        for expr in exprs {
            match expr {
                // Import statements stay at top level (not module members)
                Expr::Import { .. } => {
                    imports.push(expr);
                }
                
                // Let bindings become module members
                Expr::LetBinding { name, type_expr, value, mutable } => {
                    // Top-level let bindings are implicitly exported
                    members.push(crate::ast::TypeMember::LetBinding {
                        name,
                        type_expr: *type_expr,
                        value: *value,
                        exported: true, // File-level bindings are exported by default
                    });
                    
                    if mutable {
                        // For now, we don't support mutable module fields
                        return Err("Mutable module fields are not yet supported".to_string());
                    }
                }
                
                // If we already have a module definition, just return it
                Expr::TypeDef { .. } => {
                    // This happens when parse_module_contents already created the module
                    if imports.is_empty() && members.is_empty() {
                        return Ok(expr);
                    } else {
                        return Err("Cannot mix module definition with other top-level items".to_string());
                    }
                }
                
                // Function definitions would go here once we support top-level functions
                // For now, functions must be inside explicit module definitions
                
                // Other expressions are not allowed at top level
                _ => {
                    return Err(format!(
                        "Invalid top-level expression in module file: {:?}",
                        expr
                    ));
                }
            }
        }
        
        // If there are imports, wrap the module in a sequence with imports first
        let module_expr = Expr::TypeDef {
            name: module_name,
            members,
        };
        
        if imports.is_empty() {
            Ok(module_expr)
        } else {
            // Create sequence: imports followed by module
            let mut seq = imports;
            seq.push(module_expr);
            Ok(Expr::Sequence(seq))
        }
    }
    
    /// Compile a module and all its dependencies
    pub fn compile_module_tree(&mut self, entry_path: &Path) -> Result<Vec<CompiledModule>, String> {
        // First pass: compile all modules and build dependency graph
        let mut to_compile = vec![entry_path.to_path_buf()];
        let mut compiled_modules = HashMap::new();
        let mut visited = HashSet::new();
        
        while let Some(path) = to_compile.pop() {
            if visited.contains(&path) {
                continue;
            }
            
            let module = self.compile_file(&path)?;
            
            // Build dependency graph
            let mut deps = HashSet::new();
            for import in &module.imports {
                deps.insert(import.resolved_path.clone());
                if !visited.contains(&import.resolved_path) {
                    to_compile.push(import.resolved_path.clone());
                }
            }
            self.dependency_graph.insert(path.clone(), deps);
            
            visited.insert(path.clone());
            compiled_modules.insert(path.clone(), module);
        }
        
        // Check for circular dependencies
        self.check_circular_dependencies()?;
        
        // Topological sort to get compilation order
        let sorted_paths = self.topological_sort(entry_path)?;
        
        // Build result in dependency order
        let mut result = Vec::new();
        for path in sorted_paths {
            if let Some(module) = compiled_modules.remove(&path) {
                result.push(module);
            }
        }
        
        Ok(result)
    }
    
    /// Check for circular dependencies in the dependency graph
    fn check_circular_dependencies(&self) -> Result<(), String> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path_stack = Vec::new();
        
        for node in self.dependency_graph.keys() {
            if !visited.contains(node) {
                if self.has_cycle_dfs(node, &mut visited, &mut rec_stack, &mut path_stack)? {
                    // Build error message showing the cycle
                    let cycle_start = path_stack.iter().position(|p| p == path_stack.last().unwrap()).unwrap();
                    let cycle: Vec<String> = path_stack[cycle_start..]
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    
                    return Err(format!(
                        "Circular dependency detected: {}",
                        cycle.join(" -> ")
                    ));
                }
            }
        }
        
        Ok(())
    }
    
    /// DFS helper for cycle detection
    fn has_cycle_dfs(
        &self,
        node: &PathBuf,
        visited: &mut HashSet<PathBuf>,
        rec_stack: &mut HashSet<PathBuf>,
        path_stack: &mut Vec<PathBuf>,
    ) -> Result<bool, String> {
        visited.insert(node.clone());
        rec_stack.insert(node.clone());
        path_stack.push(node.clone());
        
        if let Some(deps) = self.dependency_graph.get(node) {
            for dep in deps {
                if !visited.contains(dep) {
                    if self.has_cycle_dfs(dep, visited, rec_stack, path_stack)? {
                        return Ok(true);
                    }
                } else if rec_stack.contains(dep) {
                    // Found a cycle - add the dependency to complete the cycle
                    path_stack.push(dep.clone());
                    return Ok(true);
                }
            }
        }
        
        rec_stack.remove(node);
        path_stack.pop();
        Ok(false)
    }
    
    /// Topological sort of the dependency graph
    fn topological_sort(&self, entry_path: &Path) -> Result<Vec<PathBuf>, String> {
        let mut visited = HashSet::new();
        let mut stack = Vec::new();
        
        // Start from entry point to ensure it's last in the result
        self.topological_sort_dfs(&entry_path.to_path_buf(), &mut visited, &mut stack)?;
        
        // Visit any remaining nodes (in case of disconnected components)
        for node in self.dependency_graph.keys() {
            if !visited.contains(node) {
                self.topological_sort_dfs(node, &mut visited, &mut stack)?;
            }
        }
        
        // Reverse to get dependency order (dependencies first)
        stack.reverse();
        Ok(stack)
    }
    
    /// DFS helper for topological sort
    fn topological_sort_dfs(
        &self,
        node: &PathBuf,
        visited: &mut HashSet<PathBuf>,
        stack: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        if visited.contains(node) {
            return Ok(());
        }
        
        visited.insert(node.clone());
        
        if let Some(deps) = self.dependency_graph.get(node) {
            for dep in deps {
                self.topological_sort_dfs(dep, visited, stack)?;
            }
        }
        
        stack.push(node.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_import_path_resolution() {
        use std::fs;
        use std::env;
        
        // Create a temporary directory structure
        let temp_dir = env::temp_dir().join("thoxa_test_import");
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        
        // Create empty test files
        fs::write(src_dir.join("main.thx"), "").unwrap();
        fs::write(src_dir.join("math.thx"), "").unwrap();
        fs::write(src_dir.join("helpers.thx"), "").unwrap();
        fs::write(temp_dir.join("utils.thx"), "").unwrap();
        
        let compiler = ModuleCompiler::new();
        let current_file = &src_dir.join("main.thx");
        
        // Test relative import
        let resolved = compiler.resolve_import_path("./math", current_file);
        assert!(resolved.is_ok(), "Failed to resolve import: {:?}", resolved.err());
        let path = resolved.unwrap();
        assert!(path.ends_with("math.thx"));
        
        // Test parent directory import
        let resolved = compiler.resolve_import_path("../utils", current_file);
        assert!(resolved.is_ok());
        
        // Test local import
        let resolved = compiler.resolve_import_path("helpers", current_file);
        assert!(resolved.is_ok());
        let path = resolved.unwrap();
        assert!(path.ends_with("helpers.thx"));
        
        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }
}