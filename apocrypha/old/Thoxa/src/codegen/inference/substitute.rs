use super::unify::Substitution;
use crate::ast::{Module, Item, Function, Stmt, Expr, Type};

/// Applies type substitutions to an AST module
pub struct TypeSubstitutor {
    substitution: Substitution,
}

impl TypeSubstitutor {
    pub fn new(substitution: Substitution) -> Self {
        Self { substitution }
    }
    
    /// Apply type substitutions to a module
    pub fn substitute_module(&self, module: &mut Module) {
        for item in &mut module.items {
            self.substitute_item(item);
        }
    }
    
    fn substitute_item(&self, item: &mut Item) {
        match item {
            Item::Function(func) => self.substitute_function(func),
            Item::Unit(_) | Item::Union(_) | Item::Type(_) | Item::Interface(_) => {
                // Type definitions don't need substitution
            }
        }
    }
    
    fn substitute_function(&self, func: &mut Function) {
        // Substitute return type if needed
        if let Some(ref mut ret_type) = func.return_type {
            self.substitute_type(ret_type);
        }
        
        // Substitute parameter types
        for param in &mut func.params {
            self.substitute_type(&mut param.ty);
        }
        
        // Substitute types in function body
        self.substitute_block(&mut func.body.stmts);
    }
    
    fn substitute_block(&self, stmts: &mut Vec<Stmt>) {
        for stmt in stmts {
            self.substitute_statement(stmt);
        }
    }
    
    fn substitute_statement(&self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let { ty, value, .. } => {
                // If type is not explicitly specified, try to infer it
                if ty.is_none() && value.is_some() {
                    // The type should have been inferred and stored somewhere
                    // For now, we'll leave it as None and let the normal codegen handle it
                }
                
                if let Some(init_expr) = value {
                    self.substitute_expression(init_expr);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    self.substitute_expression(e);
                }
            }
            Stmt::Break { value, .. } => {
                if let Some(e) = value {
                    self.substitute_expression(e);
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::Expr(expr) => {
                self.substitute_expression(expr);
            }
            Stmt::Method { body, .. } => {
                self.substitute_block(&mut body.stmts);
            }
        }
    }
    
    fn substitute_expression(&self, expr: &mut Expr) {
        match expr {
            Expr::Call { args, type_args, .. } => {
                // If no type arguments are provided but function is generic,
                // they should have been inferred
                if type_args.is_empty() {
                    // The inferred types would be stored in the substitution
                    // For now, we'll leave it for the normal instantiation process
                }
                
                for arg in args {
                    self.substitute_expression(arg);
                }
            }
            Expr::Binary { left, right, .. } => {
                self.substitute_expression(left);
                self.substitute_expression(right);
            }
            Expr::Unary { expr, .. } => {
                self.substitute_expression(expr);
            }
            Expr::FieldAccess { object, .. } => {
                self.substitute_expression(object);
            }
            Expr::Index { array, index, .. } => {
                self.substitute_expression(array);
                self.substitute_expression(index);
            }
            Expr::ArrayLiteral { elements, .. } => {
                for elem in elements {
                    self.substitute_expression(elem);
                }
            }
            Expr::UnitConstruction { fields, .. } => {
                for (_, expr) in fields {
                    self.substitute_expression(expr);
                }
            }
            Expr::If { condition, then_block, else_block, .. } => {
                self.substitute_expression(condition);
                self.substitute_block(&mut then_block.stmts);
                if let Some(else_b) = else_block {
                    self.substitute_block(&mut else_b.stmts);
                }
            }
            Expr::While { condition, body, .. } => {
                self.substitute_expression(condition);
                self.substitute_block(&mut body.stmts);
            }
            Expr::ForIn { iterable, body, .. } => {
                self.substitute_expression(iterable);
                self.substitute_block(&mut body.stmts);
            }
            Expr::Match { expr, arms, .. } => {
                self.substitute_expression(expr);
                for arm in arms {
                    self.substitute_block(&mut arm.body.stmts);
                }
            }
            Expr::Try { expr, .. } => {
                self.substitute_expression(expr);
            }
            Expr::Block(block) => {
                self.substitute_block(&mut block.stmts);
            }
            Expr::As { expr, ty, .. } => {
                self.substitute_expression(expr);
                self.substitute_type(ty);
            }
            Expr::Loop { body, .. } => {
                self.substitute_block(&mut body.stmts);
            }
            Expr::Assign { target, value, .. } => {
                self.substitute_expression(target);
                self.substitute_expression(value);
            }
            Expr::Tuple { elements, .. } => {
                for elem in elements {
                    self.substitute_expression(elem);
                }
            }
            Expr::TupleAccess { tuple, .. } => {
                self.substitute_expression(tuple);
            }
            Expr::Ident { .. } | 
            Expr::IntLiteral { .. } |
            Expr::FloatLiteral { .. } |
            Expr::StringLiteral { .. } |
            Expr::CharLiteral { .. } |
            Expr::BoolLiteral { .. } |
            Expr::VoidLiteral { .. } |
            Expr::InterpolatedString { .. } |
            Expr::EnumVariant { .. } => {}
        }
    }
    
    fn substitute_type(&self, ty: &mut Type) {
        // For now, types in the AST are already concrete
        // The substitution mainly affects type variables during inference
        // This would be enhanced to actually apply substitutions
        // when we have better integration between AST types and inference types
    }
}