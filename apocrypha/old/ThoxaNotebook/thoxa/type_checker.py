"""Type checking for Thoxa language."""

from typing import Dict, List, Optional, Set
from dataclasses import dataclass
from .ast_nodes import *


@dataclass
class TypeInfo:
    """Type information for a symbol."""
    name: str
    type_annotation: Optional[str]
    is_mutable: bool = False


class TypeChecker:
    """Static type checker for Thoxa."""
    
    def __init__(self):
        self.errors: List[str] = []
        self.warnings: List[str] = []
        self.current_scope: Dict[str, TypeInfo] = {}
        self.function_signatures: Dict[str, List[TypeInfo]] = {}
        
    def check_program(self, program: Program) -> bool:
        """Check types in a program."""
        self.errors = []
        self.warnings = []
        
        for stmt in program.statements:
            self.check_statement(stmt)
        
        return len(self.errors) == 0
    
    def check_statement(self, stmt: ASTNode):
        """Check a statement."""
        if isinstance(stmt, Function):
            self.check_function(stmt)
        elif isinstance(stmt, TypeDef):
            self.check_type_def(stmt)
        elif isinstance(stmt, Let):
            self.check_let(stmt)
        # Add more statement types as needed
    
    def check_function(self, func: Function):
        """Check function definition."""
        # Check that all parameters have type annotations
        for param in func.params:
            param_name = param[0] if isinstance(param, tuple) else param
            param_type = param[1] if isinstance(param, tuple) and len(param) > 1 else None
            
            if param_type is None or param_type == '':
                self.errors.append(
                    f"Function '{func.name}' parameter '{param_name}' requires a type annotation"
                )
        
        # Store function signature for later checking
        param_types = []
        for param in func.params:
            if isinstance(param, tuple):
                param_types.append(TypeInfo(
                    name=param[0],
                    type_annotation=param[1] if len(param) > 1 else None
                ))
        self.function_signatures[func.name] = param_types
        
        # TODO: Check function body with parameter types in scope
    
    def check_type_def(self, type_def: TypeDef):
        """Check type definition."""
        # Check constructor parameters have types
        if type_def.constructor_params:
            for param in type_def.constructor_params:
                param_name = param[0] if isinstance(param, tuple) else param
                param_type = param[1] if isinstance(param, tuple) and len(param) > 1 else None
                
                if param_type is None or param_type == '':
                    self.errors.append(
                        f"Type '{type_def.name}' constructor parameter '{param_name}' requires a type annotation"
                    )
    
    def check_let(self, let_stmt: Let):
        """Check let statement."""
        # Type annotation is optional for let statements with inference
        # But we can still check if provided
        if let_stmt.type_annotation:
            # TODO: Check that inferred type matches annotation
            pass
        
        # Add to current scope
        self.current_scope[let_stmt.name] = TypeInfo(
            name=let_stmt.name,
            type_annotation=let_stmt.type_annotation,
            is_mutable=let_stmt.mutable
        )
    
    def get_errors(self) -> List[str]:
        """Get all type errors."""
        return self.errors
    
    def get_warnings(self) -> List[str]:
        """Get all type warnings."""
        return self.warnings