"""Thoxa interpreter using visitor pattern."""

from typing import Any, Dict, Optional
from dataclasses import dataclass
from .ast_nodes import *


class BreakException(Exception):
    """Exception for break statements."""
    pass


class ContinueException(Exception):
    """Exception for continue statements."""
    pass


class ReturnException(Exception):
    """Exception for return statements."""
    def __init__(self, value):
        self.value = value


@dataclass
class Environment:
    """Variable environment for scoping."""
    parent: Optional['Environment'] = None
    values: Dict[str, Any] = None
    mutable: Dict[str, bool] = None
    
    def __post_init__(self):
        if self.values is None:
            self.values = {}
        if self.mutable is None:
            self.mutable = {}
    
    def define(self, name: str, value: Any, is_mutable: bool = False):
        """Define a new variable."""
        if name in self.values:
            raise RuntimeError(f"Variable '{name}' already defined")
        self.values[name] = value
        self.mutable[name] = is_mutable
    
    def get(self, name: str) -> Any:
        """Get variable value."""
        if name in self.values:
            return self.values[name]
        if self.parent:
            return self.parent.get(name)
        raise RuntimeError(f"Undefined variable '{name}'")
    
    def set(self, name: str, value: Any):
        """Set variable value."""
        if name in self.values:
            if not self.mutable.get(name, False):
                raise RuntimeError(f"Cannot assign to immutable variable '{name}'")
            self.values[name] = value
            return
        if self.parent:
            self.parent.set(name, value)
            return
        raise RuntimeError(f"Undefined variable '{name}'")
    
    def exists(self, name: str) -> bool:
        """Check if variable exists."""
        return name in self.values or (self.parent and self.parent.exists(name))


class Interpreter:
    """Tree-walking interpreter for Thoxa."""
    
    def __init__(self):
        self.globals = Environment()
        self.env = self.globals
        self._setup_builtins()
    
    def _setup_builtins(self):
        """Set up built-in functions."""
        self.globals.define("print", lambda *args: print(*args, sep=' ', end=''))
        self.globals.define("println", lambda *args: print(*args, sep=' '))
        self.globals.define("printInt", lambda n: print(n, end=''))
        self.globals.define("printlnInt", lambda n: print(n))
        self.globals.define("len", lambda obj: len(obj))
        self.globals.define("type", lambda obj: type(obj).__name__)
        
        def assert_fn(condition, message=None):
            if not condition:
                if message:
                    raise AssertionError(f"Assertion failed: {message}")
                else:
                    raise AssertionError("Assertion failed")
        
        self.globals.define("assert", assert_fn)
    
    
    def interpret(self, node: ASTNode) -> Any:
        """Interpret an AST node."""
        return node.accept(self)
    
    def generic_visit(self, node: ASTNode):
        """Fallback visitor method."""
        raise RuntimeError(f"No visit method for {node.node_type}")
    
    # Statement visitors
    
    def visit_program(self, node: Program) -> Any:
        """Visit program node."""
        result = None
        for stmt in node.statements:
            result = self.interpret(stmt)
        
        return result
    
    def visit_block(self, node: Block) -> Any:
        """Visit block node with new scope."""
        # Create new environment for block scope
        prev_env = self.env
        self.env = Environment(parent=prev_env)
        
        try:
            result = None
            for i, stmt in enumerate(node.statements):
                result = self.interpret(stmt)
                # Last expression in block is the return value
                # Unless it's a statement (which returns None)
            return result
        finally:
            self.env = prev_env
    
    def visit_let(self, node: Let) -> None:
        """Visit let statement."""
        value = self.interpret(node.value)
        self.env.define(node.name, value, node.mutable)
        return None
    
    def visit_assignment(self, node: Assignment) -> None:
        """Visit assignment statement."""
        value = self.interpret(node.value)
        
        if isinstance(node.target, Identifier):
            self.env.set(node.target.name, value)
        elif isinstance(node.target, Index):
            obj = self.interpret(node.target.obj)
            idx = self.interpret(node.target.index)
            obj[idx] = value
        elif isinstance(node.target, Member):
            obj = self.interpret(node.target.obj)
            # Handle instance fields (our instances are dictionaries)
            if isinstance(obj, dict) and '__type__' in obj:
                obj[node.target.member] = value
            else:
                setattr(obj, node.target.member, value)
        else:
            raise RuntimeError(f"Invalid assignment target")
        
        return None
    
    def visit_type_def(self, node: TypeDef) -> None:
        """Visit type definition."""
        # Extract fields and methods from the body
        fields = []
        methods = {}
        
        if node.body and hasattr(node.body, 'statements'):
            for member in node.body.statements:
                if isinstance(member, tuple):
                    if member[0] == 'method':
                        method = member[1]
                        methods[method.name] = method
                    elif member[0] == 'field':
                        fields.append(member[1:])  # (name, type, value, is_mutable, is_export)
        
        # Create a constructor function that creates instances
        def constructor(*args, **kwargs):
            # Create a new object (dictionary) to represent the instance
            instance = {}
            
            # Bind constructor parameters as fields
            for i, (param_name, param_type, default) in enumerate(node.constructor_params or []):
                if i < len(args):
                    instance[param_name] = args[i]
                elif param_name in kwargs:
                    instance[param_name] = kwargs[param_name]
                elif default is not None:
                    instance[param_name] = self.interpret(default)
            
            # Initialize declared fields
            for field_info in fields:
                if len(field_info) >= 1:
                    field_name = field_info[0]
                    field_value = field_info[2] if len(field_info) > 2 else None
                    if field_value is not None:
                        # If field has an initializer that references constructor params
                        if field_name in instance:
                            # Already set by constructor param
                            pass
                        else:
                            # Create environment with constructor params for evaluating field initializers
                            init_env = Environment(parent=self.env)
                            for key, value in instance.items():
                                if not key.startswith('__'):
                                    init_env.define(key, value, is_mutable=True)
                            
                            prev_env = self.env
                            self.env = init_env
                            try:
                                instance[field_name] = self.interpret(field_value) if isinstance(field_value, ASTNode) else field_value
                            finally:
                                self.env = prev_env
                    elif field_name not in instance:
                        instance[field_name] = None
            
            # Add methods to the instance
            for method_name, method_node in methods.items():
                # Create a bound method that has access to instance fields
                def make_method(method_node, inst):
                    def bound_method(*args, **kwargs):
                        # Create environment with instance fields
                        method_env = Environment(parent=self.env)
                        
                        # Add all instance fields to the method's environment
                        for key, value in inst.items():
                            if not key.startswith('__'):
                                method_env.define(key, value, is_mutable=True)
                        
                        # Bind method parameters
                        for i, (param_name, _, default) in enumerate(method_node.params):
                            if i < len(args):
                                method_env.define(param_name, args[i])
                            elif param_name in kwargs:
                                method_env.define(param_name, kwargs[param_name])
                            elif default is not None:
                                method_env.define(param_name, self.interpret(default))
                        
                        # Execute method body
                        prev_env = self.env
                        self.env = method_env
                        try:
                            result = self.interpret(method_node.body)
                            
                            # Update instance fields with any changes
                            for key in inst.keys():
                                if not key.startswith('__') and method_env.exists(key):
                                    inst[key] = method_env.get(key)
                            
                            return result
                        except ReturnException as e:
                            # Update instance fields even on return
                            for key in inst.keys():
                                if not key.startswith('__') and method_env.exists(key):
                                    inst[key] = method_env.get(key)
                            return e.value
                        finally:
                            self.env = prev_env
                    
                    return bound_method
                
                instance[method_name] = make_method(method_node, instance)
            
            # Add type name for debugging
            instance['__type__'] = node.name
            
            return instance
        
        # Register the constructor in the environment
        self.env.define(node.name, constructor, is_mutable=False)
        return None
    
    def visit_function(self, node: Function) -> None:
        """Visit function definition."""
        # Create closure capturing current environment
        def func(*args, **kwargs):
            # Create new environment for function
            func_env = Environment(parent=self.env)
            
            # Bind parameters
            for i, (param_name, _, default) in enumerate(node.params):
                if i < len(args):
                    func_env.define(param_name, args[i])
                elif param_name in kwargs:
                    func_env.define(param_name, kwargs[param_name])
                elif default:
                    func_env.define(param_name, self.interpret(default))
                else:
                    raise RuntimeError(f"Missing argument for parameter '{param_name}'")
            
            # Execute function body
            prev_env = self.env
            self.env = func_env
            try:
                result = self.interpret(node.body)
                # If block returns a value, use it as implicit return
                return result
            except ReturnException as e:
                return e.value
            finally:
                self.env = prev_env
        
        self.env.define(node.name, func)
        return None
    
    def visit_if(self, node: If) -> Any:
        """Visit if statement/expression."""
        condition = self.interpret(node.condition)
        
        if self._is_truthy(condition):
            return self.interpret(node.then_branch)
        elif node.else_branch:
            return self.interpret(node.else_branch)
        
        return None
    
    def visit_loop(self, node: Loop) -> None:
        """Visit infinite loop."""
        while True:
            try:
                self.interpret(node.body)
            except BreakException:
                break
            except ContinueException:
                continue
        return None
    
    def visit_while(self, node: While) -> None:
        """Visit while loop."""
        while self._is_truthy(self.interpret(node.condition)):
            try:
                self.interpret(node.body)
            except BreakException:
                break
            except ContinueException:
                continue
        return None
    
    def visit_for(self, node: For) -> None:
        """Visit for-in loop."""
        iterable = self.interpret(node.iterable)
        
        # Create new environment for loop variable
        loop_env = Environment(parent=self.env)
        prev_env = self.env
        self.env = loop_env
        
        try:
            for item in iterable:
                self.env.define(node.var, item, is_mutable=False)
                try:
                    self.interpret(node.body)
                except BreakException:
                    break
                except ContinueException:
                    continue
                finally:
                    # Remove loop variable for next iteration
                    del self.env.values[node.var]
        finally:
            self.env = prev_env
        
        return None
    
    def visit_return(self, node: Return) -> None:
        """Visit return statement."""
        value = self.interpret(node.value) if node.value else None
        raise ReturnException(value)
    
    def visit_break(self, node: Break) -> None:
        """Visit break statement."""
        raise BreakException()
    
    def visit_continue(self, node: Continue) -> None:
        """Visit continue statement."""
        raise ContinueException()
    
    # Expression visitors
    
    def visit_literal(self, node: Literal) -> Any:
        """Visit literal value."""
        return node.value
    
    def visit_identifier(self, node: Identifier) -> Any:
        """Visit identifier."""
        return self.env.get(node.name)
    
    def visit_binary_op(self, node: BinaryOp) -> Any:
        """Visit binary operation."""
        left = self.interpret(node.left)
        
        # Short-circuit evaluation for logical operators
        if node.op == "or":
            if self._is_truthy(left):
                return left
            return self.interpret(node.right)
        elif node.op == "and":
            if not self._is_truthy(left):
                return left
            return self.interpret(node.right)
        
        right = self.interpret(node.right)
        
        # Arithmetic operators
        if node.op == "+":
            return left + right
        elif node.op == "-":
            return left - right
        elif node.op == "*":
            return left * right
        elif node.op == "/":
            if right == 0:
                raise RuntimeError("Division by zero")
            return left / right
        elif node.op == "%":
            return left % right
        
        # Comparison operators
        elif node.op == "==":
            return left == right
        elif node.op == "!=":
            return left != right
        elif node.op == "<":
            return left < right
        elif node.op == ">":
            return left > right
        elif node.op == "<=":
            return left <= right
        elif node.op == ">=":
            return left >= right
        
        else:
            raise RuntimeError(f"Unknown binary operator: {node.op}")
    
    def visit_unary_op(self, node: UnaryOp) -> Any:
        """Visit unary operation."""
        operand = self.interpret(node.operand)
        
        if node.op == "-":
            return -operand
        elif node.op == "not":
            return not self._is_truthy(operand)
        else:
            raise RuntimeError(f"Unknown unary operator: {node.op}")
    
    def visit_call(self, node: Call) -> Any:
        """Visit function call."""
        func = self.interpret(node.func) if isinstance(node.func, ASTNode) else self.env.get(node.func)
        args = [self.interpret(arg) for arg in node.args]
        
        # Handle built-in functions and user-defined functions
        if callable(func):
            return func(*args)
        else:
            raise RuntimeError(f"'{func}' is not a function")
    
    def visit_index(self, node: Index) -> Any:
        """Visit array indexing."""
        obj = self.interpret(node.obj)
        idx = self.interpret(node.index)
        
        try:
            return obj[idx]
        except (IndexError, KeyError) as e:
            raise RuntimeError(f"Index error: {e}")
    
    def visit_member(self, node: Member) -> Any:
        """Visit member access."""
        obj = self.interpret(node.obj)
        
        # Special case for array.count
        if isinstance(obj, list) and node.member == "count":
            return len(obj)
        
        # Handle instance fields and methods (our instances are dictionaries)
        if isinstance(obj, dict) and '__type__' in obj:
            if node.member in obj:
                return obj[node.member]
            else:
                raise RuntimeError(f"Instance of '{obj['__type__']}' has no member '{node.member}'")
        
        try:
            return getattr(obj, node.member)
        except AttributeError:
            raise RuntimeError(f"'{type(obj).__name__}' has no member '{node.member}'")
    
    def visit_array(self, node: Array) -> list:
        """Visit array literal."""
        return [self.interpret(elem) for elem in node.elements]
    
    def _is_truthy(self, value: Any) -> bool:
        """Determine if a value is truthy."""
        if value is None:
            return False
        if isinstance(value, bool):
            return value
        if isinstance(value, (int, float)):
            return value != 0
        if isinstance(value, str):
            return len(value) > 0
        if isinstance(value, list):
            return len(value) > 0
        return True