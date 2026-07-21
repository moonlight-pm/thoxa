use super::types::{InferenceType, TypeVar, TypeScheme};
use super::constraints::ConstraintSet;
use crate::codegen::{CodeGen, Error, types::TypeId};
use crate::ast::{Module, Item, Function, Stmt, Expr, Pattern, BinaryOp, UnaryOp, Block};
use crate::lexer::Span;
use std::collections::HashMap;

/// Type environment for tracking variable types
#[derive(Debug, Clone)]
pub struct TypeEnvironment {
    bindings: HashMap<String, InferenceType>,
    schemes: HashMap<String, TypeScheme>,  // For polymorphic bindings
    level: u32,  // For let-polymorphism
}

impl TypeEnvironment {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            schemes: HashMap::new(),
            level: 0,
        }
    }
    
    pub fn bind(&mut self, name: String, ty: InferenceType) {
        self.bindings.insert(name, ty);
    }
    
    pub fn bind_scheme(&mut self, name: String, scheme: TypeScheme) {
        self.schemes.insert(name, scheme);
    }
    
    pub fn lookup(&self, name: &str) -> Option<InferenceType> {
        self.bindings.get(name).cloned()
    }
    
    pub fn lookup_scheme(&self, name: &str) -> Option<&TypeScheme> {
        self.schemes.get(name)
    }
    
    pub fn enter_scope(&mut self) {
        self.level += 1;
    }
    
    pub fn exit_scope(&mut self) {
        self.level -= 1;
    }
    
    pub fn current_level(&self) -> u32 {
        self.level
    }
}

/// Generates constraints from AST
pub struct ConstraintGenerator<'a> {
    constraints: ConstraintSet,
    env: TypeEnvironment,
    var_counter: &'a mut u32,
    codegen: &'a mut CodeGen,
    function_types: HashMap<String, InferenceType>,
}

impl<'a> ConstraintGenerator<'a> {
    pub fn new(codegen: &'a mut CodeGen, var_counter: &'a mut u32) -> Self {
        Self {
            constraints: ConstraintSet::new(),
            env: TypeEnvironment::new(),
            var_counter,
            codegen,
            function_types: HashMap::new(),
        }
    }
    
    pub fn generate_module(&mut self, module: &Module) -> Result<ConstraintSet, Error> {
        // First pass: collect function signatures
        for item in &module.items {
            if let Item::Function(func) = item {
                let func_type = self.function_signature(func)?;
                self.function_types.insert(func.name.clone(), func_type.clone());
                self.env.bind(func.name.clone(), func_type);
            }
        }
        
        // Second pass: generate constraints for function bodies
        for item in &module.items {
            match item {
                Item::Function(func) => {
                    self.generate_function(func)?;
                }
                Item::Type(_) | Item::Unit(_) | Item::Union(_) | Item::Interface(_) => {
                    // Type definitions don't generate constraints
                }
                _ => {}
            }
        }
        
        Ok(std::mem::take(&mut self.constraints))
    }
    
    fn function_signature(&mut self, func: &Function) -> Result<InferenceType, Error> {
        let param_types: Vec<InferenceType> = func.params.iter()
            .map(|param| {
                InferenceType::from_ast_type(&param.ty, &func.type_params)
            })
            .collect();
        
        let ret_type = if let Some(ref ty) = func.return_type {
            InferenceType::from_ast_type(ty, &func.type_params)
        } else {
            InferenceType::Concrete(TypeId::VOID)
        };
        
        Ok(InferenceType::Function {
            params: param_types,
            ret: Box::new(ret_type),
        })
    }
    
    pub fn generate_function(&mut self, func: &Function) -> Result<(), Error> {
        self.env.enter_scope();
        
        // Get function type from first pass
        let func_type = self.function_types.get(&func.name)
            .ok_or_else(|| Error::Codegen(format!("Function type not found: {}", func.name)))?
            .clone();
        
        // Extract parameter and return types
        let (param_types, ret_type) = match func_type {
            InferenceType::Function { params, ret } => (params, *ret),
            _ => return Err(Error::Codegen("Invalid function type".to_string())),
        };
        
        // Bind parameters to environment
        for (param, param_type) in func.params.iter().zip(param_types.iter()) {
            self.env.bind(param.name.clone(), param_type.clone());
        }
        
        // Generate constraints for function body
        for (i, stmt) in func.body.stmts.iter().enumerate() {
            self.generate_stmt(stmt)?;
        }
        
        // If there's an explicit return expression, constrain it
        if let Some(ref ret_expr) = func.body.expr {
            let ret_expr_type = self.generate_expr(ret_expr)?;
            self.constraints.add_equal(ret_expr_type, ret_type.clone(), ret_expr.span());
        } else if ret_type != InferenceType::Concrete(TypeId::VOID) {
            // Function should return something but doesn't
            return Err(Error::Codegen(format!(
                "Function '{}' must return a value",
                func.name
            )));
        }
        
        self.env.exit_scope();
        Ok(())
    }
    
    pub fn generate_stmt(&mut self, stmt: &Stmt) -> Result<(), Error> {
        match stmt {
            Stmt::Let { name, value, ty, span, .. } => {
                let value_type = if let Some(value) = value {
                    let vt = self.generate_expr(value)?;
                    vt
                } else {
                    InferenceType::Variable(self.fresh_type_var())
                };
                
                let var_type = if let Some(ty) = ty {
                    let expected = InferenceType::from_ast_type(ty, &[]);
                    self.constraints.add_equal(value_type.clone(), expected.clone(), *span);
                    expected
                } else {
                    value_type.clone()
                };
                
                self.env.bind(name.clone(), var_type);
                Ok(())
            }
            
            Stmt::Expr(expr) => {
                self.generate_expr(expr)?;
                Ok(())
            }
            
            Stmt::Method { .. } => {
                // Methods handled separately
                Ok(())
            }
            
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    let _value_type = self.generate_expr(value)?;
                    // Return type constraint is handled in generate_function
                }
                Ok(())
            }
            
            Stmt::Break { value, .. } => {
                if let Some(value) = value {
                    let _value_type = self.generate_expr(value)?;
                    // Break value type is handled by loop context
                }
                Ok(())
            }
            
            Stmt::Continue { .. } => Ok(()),
        }
    }
    
    pub fn generate_expr(&mut self, expr: &Expr) -> Result<InferenceType, Error> {
        match expr {
            Expr::IntLiteral { .. } => Ok(InferenceType::Concrete(TypeId::INT)),
            Expr::FloatLiteral { .. } => Ok(InferenceType::Concrete(TypeId::FLOAT)),
            Expr::StringLiteral { .. } => Ok(InferenceType::Concrete(TypeId::STRING)),
            Expr::CharLiteral { .. } => Ok(InferenceType::Concrete(TypeId::CHAR)),
            Expr::BoolLiteral { .. } => Ok(InferenceType::Concrete(TypeId::BOOL)),
            Expr::VoidLiteral { .. } => Ok(InferenceType::Concrete(TypeId::VOID)),
            
            Expr::Ident { name, span } => {
                self.env.lookup(name)
                    .or_else(|| {
                        // Check if it's a function
                        self.function_types.get(name).cloned()
                    })
                    .ok_or_else(|| Error::Codegen(format!("Undefined variable: {}", name)))
            }
            
            Expr::Binary { left, op, right, span } => {
                let left_type = self.generate_expr(left)?;
                let right_type = self.generate_expr(right)?;
                
                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                        // Numeric operations
                        self.constraints.add_numeric(left_type.clone(), *span);
                        self.constraints.add_equal(left_type.clone(), right_type, *span);
                        Ok(left_type)
                    }
                    
                    BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | 
                    BinaryOp::Gt | BinaryOp::Ge => {
                        // Comparison operations
                        self.constraints.add_comparable(left_type.clone(), *span);
                        self.constraints.add_equal(left_type, right_type, *span);
                        Ok(InferenceType::Concrete(TypeId::BOOL))
                    }
                    
                    BinaryOp::And | BinaryOp::Or => {
                        // Logical operations
                        self.constraints.add_equal(
                            left_type,
                            InferenceType::Concrete(TypeId::BOOL),
                            *span
                        );
                        self.constraints.add_equal(
                            right_type,
                            InferenceType::Concrete(TypeId::BOOL),
                            *span
                        );
                        Ok(InferenceType::Concrete(TypeId::BOOL))
                    }
                }
            }
            
            Expr::Unary { op, expr, span } => {
                let operand_type = self.generate_expr(expr)?;
                
                match op {
                    UnaryOp::Neg => {
                        self.constraints.add_numeric(operand_type.clone(), *span);
                        Ok(operand_type)
                    }
                    UnaryOp::Not => {
                        self.constraints.add_equal(
                            operand_type,
                            InferenceType::Concrete(TypeId::BOOL),
                            *span
                        );
                        Ok(InferenceType::Concrete(TypeId::BOOL))
                    }
                }
            }
            
            Expr::Call { func, args, type_args, span } => {
                let func_type = self.generate_expr(func)?;
                let arg_types: Vec<InferenceType> = args.iter()
                    .map(|arg| self.generate_expr(arg))
                    .collect::<Result<_, _>>()?;
                
                // Create fresh type variable for result
                let result_type = InferenceType::Variable(self.fresh_type_var());
                
                // Constrain function type
                let expected_func_type = InferenceType::Function {
                    params: arg_types,
                    ret: Box::new(result_type.clone()),
                };
                
                self.constraints.add_equal(func_type, expected_func_type, *span);
                Ok(result_type)
            }
            
            Expr::If { condition, then_block, else_block, span } => {
                let cond_type = self.generate_expr(condition)?;
                self.constraints.add_equal(
                    cond_type,
                    InferenceType::Concrete(TypeId::BOOL),
                    condition.span()
                );
                
                let then_type = self.generate_block_expr(then_block)?;
                
                if let Some(else_block) = else_block {
                    let else_type = self.generate_block_expr(else_block)?;
                    self.constraints.add_equal(then_type.clone(), else_type, *span);
                    Ok(then_type)
                } else {
                    // If without else returns Void
                    self.constraints.add_equal(
                        then_type,
                        InferenceType::Concrete(TypeId::VOID),
                        *span
                    );
                    Ok(InferenceType::Concrete(TypeId::VOID))
                }
            }
            
            Expr::Block(block) => {
                self.generate_block_expr(block)
            }
            
            Expr::ArrayLiteral { elements, span } => {
                if elements.is_empty() {
                    // Empty array - need type annotation or context
                    let elem_type = InferenceType::Variable(self.fresh_type_var());
                    Ok(InferenceType::Array(Box::new(elem_type)))
                } else {
                    let first_type = self.generate_expr(&elements[0])?;
                    
                    // All elements must have same type
                    for elem in &elements[1..] {
                        let elem_type = self.generate_expr(elem)?;
                        self.constraints.add_equal(first_type.clone(), elem_type, elem.span());
                    }
                    
                    Ok(InferenceType::Array(Box::new(first_type)))
                }
            }
            
            Expr::Index { array, index, span } => {
                let array_type = self.generate_expr(array)?;
                let index_type = self.generate_expr(index)?;
                
                // Index must be Int
                self.constraints.add_equal(
                    index_type,
                    InferenceType::Concrete(TypeId::INT),
                    index.span()
                );
                
                // Array type constraint
                let elem_type = InferenceType::Variable(self.fresh_type_var());
                self.constraints.add_array(array_type, elem_type.clone(), *span);
                
                Ok(elem_type)
            }
            
            Expr::FieldAccess { object, field, span } => {
                let object_type = self.generate_expr(object)?;
                let field_type = InferenceType::Variable(self.fresh_type_var());
                
                self.constraints.add_field(
                    object_type,
                    field.clone(),
                    field_type.clone(),
                    *span
                );
                
                Ok(field_type)
            }
            
            Expr::UnitConstruction { name, fields, span } => {
                // Look up constructor type
                // For now, create a fresh type variable
                let constructor_type = InferenceType::Variable(self.fresh_type_var());
                
                // Add field constraints
                for (field_name, field_expr) in fields {
                    let field_type = self.generate_expr(field_expr)?;
                    self.constraints.add_field(
                        constructor_type.clone(),
                        field_name.clone(),
                        field_type,
                        field_expr.span()
                    );
                }
                
                Ok(constructor_type)
            }
            
            Expr::Match { expr, arms, span } => {
                let expr_type = self.generate_expr(expr)?;
                
                // All arms must have same result type
                let result_type = InferenceType::Variable(self.fresh_type_var());
                
                for arm in arms {
                    // Generate pattern constraints
                    self.generate_pattern(&arm.pattern, &expr_type)?;
                    
                    // Generate arm expression
                    // Generate constraints for arm body
                    let arm_type = self.generate_block_expr(&arm.body)?;
                    self.constraints.add_equal(result_type.clone(), arm_type, arm.span);
                }
                
                Ok(result_type)
            }
            
            Expr::Loop { body, span } => {
                // Loop body is a block
                // Generate constraints for while body
                for stmt in &body.stmts {
                    self.generate_stmt(stmt)?;
                }
                let body_type = if let Some(ref expr) = body.expr {
                    self.generate_expr(expr)?
                } else {
                    InferenceType::Concrete(TypeId::VOID)
                };
                
                // Loop expression type is determined by break values
                // For now, return a fresh type variable
                Ok(InferenceType::Variable(self.fresh_type_var()))
            }
            
            Expr::ForIn { variable, iterable, body, span } => {
                let iter_type = self.generate_expr(iterable)?;
                
                // Iterable must be an array
                let elem_type = InferenceType::Variable(self.fresh_type_var());
                self.constraints.add_array(iter_type, elem_type.clone(), iterable.span());
                
                // Bind loop variable
                self.env.enter_scope();
                self.env.bind(variable.clone(), elem_type);
                
                // Generate body constraints
                for stmt in &body.stmts {
                    self.generate_stmt(stmt)?;
                }
                
                self.env.exit_scope();
                
                // For loops return Void
                Ok(InferenceType::Concrete(TypeId::VOID))
            }
            
            Expr::Try { expr, span } => {
                // Try operator - needs Trial type
                let expr_type = self.generate_expr(expr)?;
                
                let pass_type = InferenceType::Variable(self.fresh_type_var());
                let fail_type = InferenceType::Variable(self.fresh_type_var());
                
                self.constraints.add_trial(expr_type, pass_type.clone(), fail_type, *span);
                
                Ok(pass_type)
            }
            
            _ => {
                // Other expression types not yet handled
                Ok(InferenceType::Variable(self.fresh_type_var()))
            }
        }
    }
    
    fn generate_block_expr(&mut self, block: &Block) -> Result<InferenceType, Error> {
        // Generate constraints for statements
        for stmt in &block.stmts {
            self.generate_stmt(stmt)?;
        }
        
        // If there's a final expression, that's the block's type
        if let Some(ref expr) = block.expr {
            self.generate_expr(expr)
        } else {
            Ok(InferenceType::Concrete(TypeId::VOID))
        }
    }
    
    
    fn generate_pattern(&mut self, pattern: &Pattern, expr_type: &InferenceType) -> Result<(), Error> {
        match pattern {
            Pattern::Ident { name, .. } => {
                // Bind variable to expression type
                self.env.bind(name.clone(), expr_type.clone());
                Ok(())
            }
            
            Pattern::Constructor { name, fields, .. } => {
                // Handle constructor patterns (Pass, Fail, Has, Not, etc.)
                let fields = fields.as_ref().map(|v| v.as_slice()).unwrap_or(&[]);
                match name.as_str() {
                    "Pass" => {
                        if fields.len() != 1 {
                            return Err(Error::Codegen("Pass expects 1 field".to_string()));
                        }
                        let pass_type = InferenceType::Variable(self.fresh_type_var());
                        let fail_type = InferenceType::Variable(self.fresh_type_var());
                        
                        self.constraints.add_trial(
                            expr_type.clone(),
                            pass_type.clone(),
                            fail_type,
                            Span { start: 0, end: 0, line: 0, column: 0 }
                        );
                        
                        if let Some(ref pattern) = fields[0].pattern {
                            self.generate_pattern(pattern, &pass_type)?;
                        }
                    }
                    
                    "Fail" => {
                        if fields.len() != 1 {
                            return Err(Error::Codegen("Fail expects 1 field".to_string()));
                        }
                        let pass_type = InferenceType::Variable(self.fresh_type_var());
                        let fail_type = InferenceType::Variable(self.fresh_type_var());
                        
                        self.constraints.add_trial(
                            expr_type.clone(),
                            pass_type,
                            fail_type.clone(),
                            Span { start: 0, end: 0, line: 0, column: 0 }
                        );
                        
                        if let Some(ref pattern) = fields[0].pattern {
                            self.generate_pattern(pattern, &fail_type)?;
                        }
                    }
                    
                    "Has" => {
                        if fields.len() != 1 {
                            return Err(Error::Codegen("Has expects 1 field".to_string()));
                        }
                        let inner_type = InferenceType::Variable(self.fresh_type_var());
                        
                        self.constraints.add_may(
                            expr_type.clone(),
                            inner_type.clone(),
                            Span { start: 0, end: 0, line: 0, column: 0 }
                        );
                        
                        if let Some(ref pattern) = fields[0].pattern {
                            self.generate_pattern(pattern, &inner_type)?;
                        }
                    }
                    
                    "Not" => {
                        if !fields.is_empty() {
                            return Err(Error::Codegen("Not expects no fields".to_string()));
                        }
                        let inner_type = InferenceType::Variable(self.fresh_type_var());
                        
                        self.constraints.add_may(
                            expr_type.clone(),
                            inner_type,
                            Span { start: 0, end: 0, line: 0, column: 0 }
                        );
                    }
                    
                    _ => {
                        // Custom constructor - add field constraints
                        for field in fields {
                            if let Some(ref pattern) = field.pattern {
                                let field_type = InferenceType::Variable(self.fresh_type_var());
                                self.generate_pattern(pattern, &field_type)?;
                            }
                        }
                    }
                }
                Ok(())
            }
            
            Pattern::IntLiteral { .. } => {
                self.constraints.add_equal(expr_type.clone(), InferenceType::Concrete(TypeId::INT), Span { start: 0, end: 0, line: 0, column: 0 });
                Ok(())
            }
            Pattern::StringLiteral { .. } => {
                self.constraints.add_equal(expr_type.clone(), InferenceType::Concrete(TypeId::STRING), Span { start: 0, end: 0, line: 0, column: 0 });
                Ok(())
            }
            Pattern::BoolLiteral { .. } => {
                self.constraints.add_equal(expr_type.clone(), InferenceType::Concrete(TypeId::BOOL), Span { start: 0, end: 0, line: 0, column: 0 });
                Ok(())
            }
            
            Pattern::Wildcard { .. } => {
                // Wildcard matches anything
                Ok(())
            }
            
            _ => Ok(()),
        }
    }
    
    fn fresh_type_var(&mut self) -> TypeVar {
        let var = TypeVar {
            id: *self.var_counter,
            name: None,
            level: self.env.current_level(),
        };
        *self.var_counter += 1;
        var
    }
}