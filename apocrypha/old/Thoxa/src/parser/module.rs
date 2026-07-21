use crate::ast::{Module, Import, ImportItem, Item, Interface, InterfaceMethod, UnitDef, UnionDef, Param, Stmt};
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use super::Parser;

impl Parser {
    /// Parse a complete module
    pub fn parse_module(&mut self) -> Result<Module> {
        let start = self.current_span();
        let mut imports = Vec::new();
        let mut items = Vec::new();
        
        // Parse imports first
        while self.at(TokenKind::Import) {
            imports.push(self.parse_import()?);
        }
        
        // Parse top-level items
        while !self.at_eof() {
            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(e) => {
                    self.error(e);
                    self.synchronize();
                }
            }
        }
        
        // Return any errors collected
        if !self.errors.is_empty() {
            return Err(self.make_error());
        }
        
        Ok(Module {
            imports,
            items,
            span: self.span_from(start),
        })
    }
    
    /// Parse an import statement
    /// import { foo, bar as baz } from "module"
    pub(super) fn parse_import(&mut self) -> Result<Import> {
        let start = self.current_span();
        self.expect(TokenKind::Import)?;
        
        self.expect(TokenKind::LeftBrace)?;
        
        let mut items = Vec::new();
        
        while !self.at(TokenKind::RightBrace) && !self.at_eof() {
            // Parse import item
            let name = match &self.current.kind {
                TokenKind::Identifier(n) => {
                    let name = n.clone();
                    self.advance()?;
                    name
                }
                _ => return Err(Error::Parse("Expected identifier in import list".to_string())),
            };
            
            // Check for 'as' rename
            let item = if self.at(TokenKind::As) {
                self.advance()?; // consume 'as'
                
                let as_name = match &self.current.kind {
                    TokenKind::Identifier(n) => {
                        let as_name = n.clone();
                        self.advance()?;
                        as_name
                    }
                    _ => return Err(Error::Parse("Expected identifier after 'as'".to_string())),
                };
                
                ImportItem::Renamed { name, as_name }
            } else {
                ImportItem::Named(name)
            };
            
            items.push(item);
            
            if !self.at(TokenKind::RightBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightBrace)?;
        self.expect(TokenKind::From)?;
        
        // Parse module path (string literal)
        let from = match &self.current.kind {
            TokenKind::StringLiteral(path) => {
                let path = path.clone();
                self.advance()?;
                path
            }
            _ => return Err(Error::Parse("Expected string literal for module path".to_string())),
        };
        
        // Optional semicolon
        if self.at(TokenKind::Semicolon) {
            self.advance()?;
        }
        
        Ok(Import {
            items,
            from,
            span: self.span_from(start),
        })
    }
    
    /// Parse a top-level item (function, type, interface)
    pub(super) fn parse_item(&mut self) -> Result<Item> {
        let is_export = if self.at(TokenKind::Export) {
            self.advance()?;
            true
        } else {
            false
        };
        
        match &self.current.kind {
            TokenKind::Fn => self.parse_fn(is_export),
            TokenKind::Interface => self.parse_interface(is_export),
            TokenKind::Type => self.parse_unit(is_export),
            _ => Err(Error::Parse("Expected 'fn', 'interface', or 'type'".to_string())),
        }
    }
    
    /// Parse an interface definition
    fn parse_interface(&mut self, is_export: bool) -> Result<Item> {
        let start = self.current_span();
        self.expect(TokenKind::Interface)?;
        
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            _ => return Err(Error::Parse("Expected interface name".to_string())),
        };
        
        // TODO: Parse generic parameters if present
        
        self.expect(TokenKind::LeftBrace)?;
        
        let mut methods = Vec::new();
        
        while !self.at(TokenKind::RightBrace) && !self.at_eof() {
            let method_start = self.current_span();
            
            // Parse method name
            let method_name = match &self.current.kind {
                TokenKind::Identifier(n) => {
                    let name = n.clone();
                    self.advance()?;
                    name
                }
                _ => return Err(Error::Parse("Expected method name".to_string())),
            };
            
            // Parse method parameters
            self.expect(TokenKind::LeftParen)?;
            
            let mut params = Vec::new();
            while !self.at(TokenKind::RightParen) && !self.at_eof() {
                params.push(self.parse_param()?);
                
                if !self.at(TokenKind::RightParen) {
                    self.expect(TokenKind::Comma)?;
                }
            }
            
            self.expect(TokenKind::RightParen)?;
            
            // Parse optional return type
            let return_type = if self.at(TokenKind::Arrow) {
                self.advance()?;
                Some(self.parse_type()?)
            } else {
                None
            };
            
            methods.push(InterfaceMethod {
                name: method_name,
                params,
                return_type,
                is_export: false, // Interface methods inherit export from interface
                span: self.span_from(method_start),
            });
            
            // Optional comma or semicolon
            if self.at(TokenKind::Comma) || self.at(TokenKind::Semicolon) {
                self.advance()?;
            }
        }
        
        self.expect(TokenKind::RightBrace)?;
        
        Ok(Item::Interface(Interface {
            name,
            methods,
            is_export,
            span: self.span_from(start),
        }))
    }
    
    /// Parse a unit definition
    /// type Person { export let name: String; export let age: Int }
    /// type Counter(initial: Int) { export let mut count = initial }
    fn parse_unit(&mut self, is_export: bool) -> Result<Item> {
        let start = self.current_span();
        self.expect(TokenKind::Type)?;
        
        // Parse unit name
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            _ => return Err(Error::Parse("Expected type name".to_string())),
        };
        
        // Parse optional generic parameters
        let type_params = if self.at(TokenKind::Less) {
            self.advance()?; // consume '<'
            let mut params = Vec::new();
            
            while !self.at(TokenKind::Greater) && !self.at_eof() {
                match &self.current.kind {
                    TokenKind::Identifier(param) => {
                        params.push(param.clone());
                        self.advance()?;
                    }
                    _ => return Err(Error::Parse("Expected type parameter name".to_string())),
                }
                
                if !self.at(TokenKind::Greater) {
                    self.expect(TokenKind::Comma)?;
                }
            }
            
            self.expect(TokenKind::Greater)?;
            params
        } else {
            Vec::new()
        };
        
        // Check for union type definition (type Shape = Circle | Rectangle)
        if self.at(TokenKind::Equal) {
            return self.parse_union_type(name, type_params, is_export, start);
        }
        
        // Parse optional constructor parameters
        let constructor_params = if self.at(TokenKind::LeftParen) {
            self.advance()?; // consume '('
            let mut params = Vec::new();
            
            while !self.at(TokenKind::RightParen) && !self.at_eof() {
                // For now, parse simple positional parameters
                // TODO: Add support for named parameters and defaults
                let param_name = match &self.current.kind {
                    TokenKind::Identifier(n) => {
                        let name = n.clone();
                        self.advance()?;
                        name
                    }
                    _ => return Err(Error::Parse("Expected parameter name".to_string())),
                };
                
                self.expect(TokenKind::Colon)?;
                let param_type = self.parse_type()?;
                
                params.push(Param {
                    name: param_name,
                    ty: param_type,
                    is_mut: false,
                    span: self.current_span(),
                });
                
                if !self.at(TokenKind::RightParen) {
                    self.expect(TokenKind::Comma)?;
                }
            }
            
            self.expect(TokenKind::RightParen)?;
            params
        } else {
            Vec::new()
        };
        
        // Parse unit body as statement list
        self.expect(TokenKind::LeftBrace)?;
        
        let mut body_stmts = Vec::new();
        
        while !self.at(TokenKind::RightBrace) && !self.at_eof() {
            // Check for export prefix (optional)
            let is_export = if self.at(TokenKind::Export) {
                self.advance()?;
                true
            } else {
                false
            };
            
            // Parse different kinds of statements
            let stmt = if self.at(TokenKind::Let) {
                // Parse let statement with export
                let start = self.current_span();
                self.advance()?; // consume 'let'
                
                // Check for 'mut' modifier
                let is_mut = if self.at(TokenKind::Mut) {
                    self.advance()?;
                    true
                } else {
                    false
                };
                
                // Parse name
                let name = match &self.current.kind {
                    TokenKind::Identifier(n) => {
                        let name = n.clone();
                        self.advance()?;
                        name
                    }
                    _ => return Err(Error::Parse("Expected identifier after 'let'".to_string())),
                };
                
                // Parse type annotation (required for now)
                self.expect(TokenKind::Colon)?;
                let ty = self.parse_type()?;
                
                // Optional initializer
                let value = if self.at(TokenKind::Equal) {
                    self.advance()?;
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                
                Stmt::Let {
                    name,
                    ty: Some(ty),
                    value,
                    is_mut,
                    is_export,
                    span: self.span_from(start),
                }
            } else if self.at(TokenKind::Fn) {
                // Parse exported function as a method
                let method_start = self.current_span();
                self.advance()?; // consume 'fn'
                
                // Parse method name
                let method_name = match &self.current.kind {
                    TokenKind::Identifier(n) => {
                        let name = n.clone();
                        self.advance()?;
                        name
                    }
                    _ => return Err(Error::Parse("Expected method name".to_string())),
                };
                
                // Parse parameters
                self.expect(TokenKind::LeftParen)?;
                let mut params = Vec::new();
                
                while !self.at(TokenKind::RightParen) && !self.at_eof() {
                    params.push(self.parse_param()?);
                    
                    if !self.at(TokenKind::RightParen) {
                        self.expect(TokenKind::Comma)?;
                    }
                }
                
                self.expect(TokenKind::RightParen)?;
                
                // Parse optional return type
                let return_type = if self.at(TokenKind::Arrow) {
                    self.advance()?; // consume '->'
                    Some(self.parse_type()?)
                } else {
                    None
                };
                
                // Parse body
                let body = self.parse_block()?;
                
                // Create a method statement
                Stmt::Method {
                    name: method_name,
                    params,
                    return_type,
                    body,
                    is_export,
                    span: self.span_from(method_start),
                }
            } else if is_export {
                // If we have export but neither let nor fn, it's an error
                return Err(Error::Parse(format!("Expected 'let' or 'fn' after 'export', found {:?}", self.current.kind)));
            } else {
                // No valid statement found
                return Err(Error::Parse(format!("Expected 'let', 'fn', or 'export' in type body, found {:?}", self.current.kind)));
            };
            
            body_stmts.push(stmt);
            
            // Statements in type bodies are separated by commas or semicolons (for now)
            // Methods don't require separators after their closing brace
            if !self.at(TokenKind::RightBrace) {
                if self.at(TokenKind::Comma) {
                    self.advance()?;
                } else if self.at(TokenKind::Semicolon) {
                    self.advance()?;
                } else if !matches!(body_stmts.last(), Some(Stmt::Method { .. })) {
                    // For non-method statements, require a separator
                    return Err(Error::Parse("Expected comma or semicolon after statement in type body".to_string()));
                }
                // Methods can appear without trailing separator
            }
        }
        
        self.expect(TokenKind::RightBrace)?;
        
        Ok(Item::Unit(UnitDef {
            name,
            type_params,
            constructor_params,
            body: body_stmts,
            is_export,
            span: self.span_from(start),
        }))
    }
    
    /// Parse a union type definition: type Shape = Circle | Rectangle
    fn parse_union_type(
        &mut self, 
        name: String, 
        type_params: Vec<String>, 
        is_export: bool, 
        start: crate::lexer::Span
    ) -> Result<Item> {
        self.expect(TokenKind::Equal)?;
        
        let mut variants = Vec::new();
        
        // Parse first variant
        variants.push(self.parse_type()?);
        
        // Parse remaining variants with | separator
        while self.at(TokenKind::Pipe) {
            self.advance()?; // consume '|'
            variants.push(self.parse_type()?);
        }
        
        Ok(Item::Union(UnionDef {
            name,
            type_params,
            variants,
            is_export,
            span: self.span_from(start),
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{ImportItem, Item};
    use crate::parser::Parser;
    
    #[test]
    fn test_simple_import() {
        let mut parser = Parser::new(r#"import { foo } from "module""#).unwrap();
        let import = parser.parse_import().unwrap();
        
        assert_eq!(import.from, "module");
        assert_eq!(import.items.len(), 1);
        match &import.items[0] {
            ImportItem::Named(name) => assert_eq!(name, "foo"),
            _ => panic!("Expected named import"),
        }
    }
    
    #[test]
    fn test_import_with_rename() {
        let mut parser = Parser::new(r#"import { foo as bar } from "module""#).unwrap();
        let import = parser.parse_import().unwrap();
        
        assert_eq!(import.from, "module");
        assert_eq!(import.items.len(), 1);
        match &import.items[0] {
            ImportItem::Renamed { name, as_name } => {
                assert_eq!(name, "foo");
                assert_eq!(as_name, "bar");
            }
            _ => panic!("Expected renamed import"),
        }
    }
    
    #[test]
    fn test_multiple_imports() {
        let mut parser = Parser::new(r#"import { foo, bar as baz, qux } from "module""#).unwrap();
        let import = parser.parse_import().unwrap();
        
        assert_eq!(import.from, "module");
        assert_eq!(import.items.len(), 3);
        
        match &import.items[0] {
            ImportItem::Named(name) => assert_eq!(name, "foo"),
            _ => panic!("Expected named import"),
        }
        
        match &import.items[1] {
            ImportItem::Renamed { name, as_name } => {
                assert_eq!(name, "bar");
                assert_eq!(as_name, "baz");
            }
            _ => panic!("Expected renamed import"),
        }
        
        match &import.items[2] {
            ImportItem::Named(name) => assert_eq!(name, "qux"),
            _ => panic!("Expected named import"),
        }
    }
    
    #[test]
    fn test_export_function() {
        let mut parser = Parser::new(r#"
            export fn hello() {
                print("Hello, world!");
            }
        "#).unwrap();
        let item = parser.parse_item().unwrap();
        
        match item {
            Item::Function(func) => {
                assert_eq!(func.name, "hello");
                assert!(func.is_export);
            }
            _ => panic!("Expected Function item"),
        }
    }
    
    
    #[test]
    fn test_interface() {
        let mut parser = Parser::new(r#"
            interface Point {
                x() -> Float,
                y() -> Float
            }
        "#).unwrap();
        let item = parser.parse_item().unwrap();
        
        match item {
            Item::Interface(iface) => {
                assert_eq!(iface.name, "Point");
                assert!(!iface.is_export);
                assert_eq!(iface.methods.len(), 2);
                assert_eq!(iface.methods[0].name, "x");
                assert_eq!(iface.methods[1].name, "y");
            }
            _ => panic!("Expected Interface item"),
        }
    }
    
    #[test]
    fn test_union_type() {
        let mut parser = Parser::new("type Shape = Circle | Rectangle").unwrap();
        let item = parser.parse_item().unwrap();
        
        match item {
            Item::Union(union) => {
                assert_eq!(union.name, "Shape");
                assert_eq!(union.variants.len(), 2);
                assert!(union.type_params.is_empty());
                assert!(!union.is_export);
            }
            _ => panic!("Expected Union item, got {:?}", item),
        }
    }
    
    #[test]
    fn test_generic_union_type() {
        let mut parser = Parser::new("type May<T> = Has<T> | Not").unwrap();
        let item = parser.parse_item().unwrap();
        
        match item {
            Item::Union(union) => {
                assert_eq!(union.name, "May");
                assert_eq!(union.type_params, vec!["T"]);
                assert_eq!(union.variants.len(), 2);
            }
            _ => panic!("Expected Union item"),
        }
    }
    
    #[test]
    fn test_complete_module() {
        let mut parser = Parser::new(r#"
            import { foo, bar as baz } from "utils"
            import { List } from "collections"
            
            export fn main() {
                print("Hello!");
            }
            
            export interface User {
                id() -> Int,
                name() -> String
            }
        "#).unwrap();
        
        let module = parser.parse_module().unwrap();
        
        assert_eq!(module.imports.len(), 2);
        assert_eq!(module.items.len(), 2); // main, User
        
        // Check imports
        assert_eq!(module.imports[0].from, "utils");
        assert_eq!(module.imports[0].items.len(), 2);
        assert_eq!(module.imports[1].from, "collections");
        assert_eq!(module.imports[1].items.len(), 1);
        
        // Check items
        match &module.items[0] {
            Item::Function(func) => {
                assert_eq!(func.name, "main");
                assert!(func.is_export);
            }
            _ => panic!("Expected Function item"),
        }
        
        match &module.items[1] {
            Item::Interface(iface) => {
                assert_eq!(iface.name, "User");
                assert!(iface.is_export);
            }
            _ => panic!("Expected Interface item"),
        }
    }
}