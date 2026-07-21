//! Operator parsing with precedence handling

use crate::ast::{Expr, BinaryOp, UnaryOp};
use crate::error::Result;
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse logical OR (lowest precedence)
    pub(in crate::parser) fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;
        
        while self.at(TokenKind::Or) {
            self.advance()?; // consume 'or'
            let right = self.parse_and()?;
            let span = self.span_from(expr.span());
            
            expr = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        
        Ok(expr)
    }
    
    /// Parse logical AND
    pub(in crate::parser) fn parse_and(&mut self) -> Result<Expr> {
        let mut expr = self.parse_equality()?;
        
        while self.at(TokenKind::And) {
            self.advance()?; // consume 'and'
            let right = self.parse_equality()?;
            let span = self.span_from(expr.span());
            
            expr = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        
        Ok(expr)
    }
    
    /// Parse equality operators (==, !=)
    pub(in crate::parser) fn parse_equality(&mut self) -> Result<Expr> {
        let mut expr = self.parse_comparison()?;
        
        loop {
            let op = match &self.current.kind {
                TokenKind::EqualEqual => BinaryOp::Eq,
                TokenKind::NotEqual => BinaryOp::Ne,
                _ => break,
            };
            
            self.advance()?;
            let right = self.parse_comparison()?;
            let span = self.span_from(expr.span());
            
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        
        Ok(expr)
    }
    
    /// Parse comparison operators (<, <=, >, >=)
    pub(in crate::parser) fn parse_comparison(&mut self) -> Result<Expr> {
        let mut expr = self.parse_addition()?;
        
        loop {
            let op = match &self.current.kind {
                TokenKind::Less => BinaryOp::Lt,
                TokenKind::LessEqual => BinaryOp::Le,
                TokenKind::Greater => BinaryOp::Gt,
                TokenKind::GreaterEqual => BinaryOp::Ge,
                _ => break,
            };
            
            self.advance()?;
            let right = self.parse_addition()?;
            let span = self.span_from(expr.span());
            
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        
        Ok(expr)
    }
    
    /// Parse addition and subtraction
    pub(in crate::parser) fn parse_addition(&mut self) -> Result<Expr> {
        let mut expr = self.parse_multiplication()?;
        
        loop {
            let op = match &self.current.kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            
            self.advance()?;
            let right = self.parse_multiplication()?;
            let span = self.span_from(expr.span());
            
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        
        Ok(expr)
    }
    
    /// Parse multiplication, division, and modulo
    pub(in crate::parser) fn parse_multiplication(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        
        loop {
            let op = match &self.current.kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => break,
            };
            
            self.advance()?;
            let right = self.parse_unary()?;
            let span = self.span_from(expr.span());
            
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        
        Ok(expr)
    }
    
    /// Parse unary operators (-, not)
    pub(in crate::parser) fn parse_unary(&mut self) -> Result<Expr> {
        let start = self.current_span();
        
        let op = match &self.current.kind {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Not => Some(UnaryOp::Not),
            _ => None,
        };
        
        if let Some(op) = op {
            self.advance()?;
            let expr = self.parse_unary()?; // Right associative
            let span = self.span_from(start);
            
            return Ok(Expr::Unary {
                op,
                expr: Box::new(expr),
                span,
            });
        }
        
        self.parse_postfix()
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{Expr, BinaryOp, UnaryOp};
    use crate::parser::Parser;
    
    #[test]
    fn test_unary_operators() {
        // Test negation
        let mut parser = Parser::new("-42").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Unary { op: UnaryOp::Neg, expr, .. } => {
                match expr.as_ref() {
                    Expr::IntLiteral { value: 42, .. } => {},
                    _ => panic!("Expected IntLiteral 42"),
                }
            }
            _ => panic!("Expected Unary Neg"),
        }
        
        // Test not
        let mut parser = Parser::new("not true").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Unary { op: UnaryOp::Not, expr, .. } => {
                match expr.as_ref() {
                    Expr::BoolLiteral { value: true, .. } => {},
                    _ => panic!("Expected BoolLiteral true"),
                }
            }
            _ => panic!("Expected Unary Not"),
        }
    }
    
    #[test]
    fn test_binary_arithmetic() {
        let mut parser = Parser::new("1 + 2 * 3").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        // Should parse as 1 + (2 * 3) due to precedence
        match expr {
            Expr::Binary { op: BinaryOp::Add, left, right, .. } => {
                match left.as_ref() {
                    Expr::IntLiteral { value: 1, .. } => {},
                    _ => panic!("Expected left to be 1"),
                }
                match right.as_ref() {
                    Expr::Binary { op: BinaryOp::Mul, left, right, .. } => {
                        match (left.as_ref(), right.as_ref()) {
                            (Expr::IntLiteral { value: 2, .. }, 
                             Expr::IntLiteral { value: 3, .. }) => {},
                            _ => panic!("Expected 2 * 3"),
                        }
                    }
                    _ => panic!("Expected right to be multiplication"),
                }
            }
            _ => panic!("Expected addition at top level"),
        }
    }
    
    #[test]
    fn test_comparison_operators() {
        let mut parser = Parser::new("x < 5").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Binary { op: BinaryOp::Lt, left, right, .. } => {
                match (left.as_ref(), right.as_ref()) {
                    (Expr::Ident { name, .. }, Expr::IntLiteral { value: 5, .. }) => {
                        assert_eq!(name, "x");
                    }
                    _ => panic!("Expected x < 5"),
                }
            }
            _ => panic!("Expected less than comparison"),
        }
    }
    
    #[test]
    fn test_logical_operators() {
        let mut parser = Parser::new("a and b or c").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        // Should parse as (a and b) or c
        match expr {
            Expr::Binary { op: BinaryOp::Or, left, right, .. } => {
                match left.as_ref() {
                    Expr::Binary { op: BinaryOp::And, left, right, .. } => {
                        match (left.as_ref(), right.as_ref()) {
                            (Expr::Ident { name: a, .. }, 
                             Expr::Ident { name: b, .. }) => {
                                assert_eq!(a, "a");
                                assert_eq!(b, "b");
                            }
                            _ => panic!("Expected a and b"),
                        }
                    }
                    _ => panic!("Expected left to be 'and' expression"),
                }
                match right.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "c"),
                    _ => panic!("Expected right to be 'c'"),
                }
            }
            _ => panic!("Expected 'or' at top level"),
        }
    }
    
    #[test]
    fn test_operator_precedence() {
        // Test that operations group correctly
        let mut parser = Parser::new("1 + 2 == 3 and true").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        // Should parse as ((1 + 2) == 3) and true
        match expr {
            Expr::Binary { op: BinaryOp::And, left, right, .. } => {
                // Check left side: (1 + 2) == 3
                match left.as_ref() {
                    Expr::Binary { op: BinaryOp::Eq, left, right, .. } => {
                        // Check (1 + 2)
                        match left.as_ref() {
                            Expr::Binary { op: BinaryOp::Add, .. } => {},
                            _ => panic!("Expected addition"),
                        }
                        // Check 3
                        match right.as_ref() {
                            Expr::IntLiteral { value: 3, .. } => {},
                            _ => panic!("Expected 3"),
                        }
                    }
                    _ => panic!("Expected equality comparison"),
                }
                // Check right side: true
                match right.as_ref() {
                    Expr::BoolLiteral { value: true, .. } => {},
                    _ => panic!("Expected true"),
                }
            }
            _ => panic!("Expected 'and' at top level"),
        }
    }
}