# Thoxa Type System Design

## Overview

Thoxa features a static type system with runtime introspection capabilities. Types are checked at compile/parse time for safety while maintaining runtime type information for debugging, safe downcasting, and reflection.

## Core Concepts

### Static Typing with Inference

- All types must be known at compile time
- Type inference for local variables and simple expressions
- No runtime code generation or evaluation
- Functions and types require explicit type annotations

### Value Representation

In the Python implementation, all values carry their type:

```python
@dataclass
class Value:
    type: Type
    data: Any  # Python object or primitive
    
class Type:
    def __init__(self, name: str, kind: TypeKind):
        self.name = name
        self.kind = kind
        self.methods = {}
        self.fields = {}
        self.parent = None  # For inheritance
        self.interfaces = []  # Implemented interfaces
```

## Type Categories

### 1. Primitive Types

Built-in types with special handling:

```python
# In types.py
INT_TYPE = Type("Int", TypeKind.PRIMITIVE)
FLOAT_TYPE = Type("Float", TypeKind.PRIMITIVE)
BOOL_TYPE = Type("Bool", TypeKind.PRIMITIVE)
STRING_TYPE = Type("String", TypeKind.PRIMITIVE)
CHAR_TYPE = Type("Char", TypeKind.PRIMITIVE)
VOID_TYPE = Type("Void", TypeKind.PRIMITIVE)
```

### 2. Composite Types

User-defined types with fields and methods:

```python
class CompositeType(Type):
    def __init__(self, name: str, constructor_params: List[Param]):
        super().__init__(name, TypeKind.COMPOSITE)
        self.constructor_params = constructor_params
        self.initializer = None  # Constructor body
```

### 3. Union Types

Sum types representing alternatives:

```python
class UnionType(Type):
    def __init__(self, name: str, variants: List[Type]):
        super().__init__(name, TypeKind.UNION)
        self.variants = variants
    
    def accepts(self, value_type: Type) -> bool:
        return any(v.is_compatible(value_type) for v in self.variants)
```

### 4. Generic Types

Parameterized types with type variables:

```python
class GenericType(Type):
    def __init__(self, name: str, type_params: List[str]):
        super().__init__(name, TypeKind.GENERIC)
        self.type_params = type_params
        self.instances = {}  # Cache of instantiated types
    
    def instantiate(self, type_args: List[Type]) -> Type:
        key = tuple(t.name for t in type_args)
        if key not in self.instances:
            self.instances[key] = self._create_instance(type_args)
        return self.instances[key]
```

## Type Inference

### Local Variable Inference

```python
class TypeInferencer:
    def infer_literal(self, literal: Any) -> Type:
        if isinstance(literal, int):
            return INT_TYPE
        elif isinstance(literal, float):
            return FLOAT_TYPE
        elif isinstance(literal, bool):
            return BOOL_TYPE
        elif isinstance(literal, str):
            return STRING_TYPE
        else:
            raise TypeError(f"Cannot infer type for {literal}")
    
    def infer_expression(self, expr: Expression, env: Environment) -> Type:
        if isinstance(expr, Literal):
            return self.infer_literal(expr.value)
        elif isinstance(expr, BinaryOp):
            left_type = self.infer_expression(expr.left, env)
            right_type = self.infer_expression(expr.right, env)
            return self.infer_binary_op(expr.op, left_type, right_type)
        # ... more cases
```

### Generic Function Inference

```python
def infer_generic_args(func: GenericFunction, arg_types: List[Type]) -> Dict[str, Type]:
    """Infer type arguments for a generic function call."""
    type_map = {}
    
    for param, arg_type in zip(func.params, arg_types):
        if param.type.is_type_variable():
            if param.type.name in type_map:
                # Verify consistency
                if type_map[param.type.name] != arg_type:
                    raise TypeError("Inconsistent type inference")
            else:
                type_map[param.type.name] = arg_type
    
    return type_map
```

## Built-in Generic Types

### May<T> - Optional Values

```python
# Built-in definition
class MayType(GenericType):
    def __init__(self):
        super().__init__("May", ["T"])
        self.has_constructor = HasConstructor()
        self.not_constructor = NotConstructor()

# Usage in interpreter
def create_may_value(value: Optional[Value], T: Type) -> Value:
    may_type = MAY_TYPE.instantiate([T])
    if value is not None:
        return Value(may_type, {"variant": "Has", "value": value})
    else:
        return Value(may_type, {"variant": "Not"})
```

### Trial<T, E> - Error Handling

```python
class TrialType(GenericType):
    def __init__(self):
        super().__init__("Trial", ["T", "E"])
        self.pass_constructor = PassConstructor()
        self.fail_constructor = FailConstructor()

# The ? operator implementation
class QuestionMarkOperator:
    def evaluate(self, trial_value: Value, context: Context) -> Value:
        if trial_value.data["variant"] == "Fail":
            # Propagate error by raising exception
            raise TrialPropagation(trial_value)
        return trial_value.data["value"]
```

## Pattern Matching

### Type Patterns

```python
class TypePattern:
    def __init__(self, type: Type, binding: Optional[str]):
        self.type = type
        self.binding = binding
    
    def matches(self, value: Value) -> Optional[Dict[str, Value]]:
        if value.type.is_compatible(self.type):
            if self.binding:
                return {self.binding: value}
            return {}
        return None
```

### Destructuring Patterns

```python
class DestructuringPattern:
    def __init__(self, constructor: str, param_patterns: List[Pattern]):
        self.constructor = constructor
        self.param_patterns = param_patterns
    
    def matches(self, value: Value) -> Optional[Dict[str, Value]]:
        if not isinstance(value.data, dict):
            return None
        
        if value.data.get("__constructor__") != self.constructor:
            return None
        
        bindings = {}
        for pattern, param_name in zip(self.param_patterns, self.constructor_params):
            param_value = value.data.get(param_name)
            if param_match := pattern.matches(param_value):
                bindings.update(param_match)
            else:
                return None
        
        return bindings
```

## Type Checking

### Static Type Checking

```python
class TypeChecker:
    def __init__(self):
        self.errors = []
        self.environment = TypeEnvironment()
    
    def check_program(self, ast: Program) -> bool:
        for statement in ast.statements:
            self.check_statement(statement)
        return len(self.errors) == 0
    
    def check_expression(self, expr: Expression) -> Type:
        if isinstance(expr, FunctionCall):
            return self.check_function_call(expr)
        elif isinstance(expr, IfExpression):
            return self.check_if_expression(expr)
        # ... more cases
    
    def check_function_call(self, call: FunctionCall) -> Type:
        func_type = self.environment.get_function(call.name)
        arg_types = [self.check_expression(arg) for arg in call.args]
        
        # Check argument types match parameters
        for param, arg_type in zip(func_type.params, arg_types):
            if not param.type.is_compatible(arg_type):
                self.errors.append(
                    f"Type mismatch: expected {param.type}, got {arg_type}"
                )
        
        return func_type.return_type
```

### Runtime Type Information

```python
class RuntimeTypeInfo:
    """Maintains runtime type information for introspection."""
    
    def get_type_of(self, value: Value) -> Type:
        return value.type
    
    def is_instance(self, value: Value, type: Type) -> bool:
        return value.type.is_compatible(type)
    
    def safe_cast(self, value: Value, target_type: Type) -> Optional[Value]:
        if self.is_instance(value, target_type):
            return Value(target_type, value.data)
        return None
```

## Interface Implementation

### Structural Typing

Types implicitly implement interfaces by having matching methods:

```python
class Interface(Type):
    def __init__(self, name: str, methods: Dict[str, FunctionType]):
        super().__init__(name, TypeKind.INTERFACE)
        self.required_methods = methods
    
    def is_implemented_by(self, type: Type) -> bool:
        for method_name, method_type in self.required_methods.items():
            if method_name not in type.methods:
                return False
            if not method_type.is_compatible(type.methods[method_name]):
                return False
        return True
```

## Memory Model

### Automatic Memory Management

Python's garbage collector handles all memory:

```python
# No manual memory management needed
# Values are Python objects with automatic refcounting + cycle detection

def create_array(elements: List[Value]) -> Value:
    # Python list handles memory automatically
    return Value(ARRAY_TYPE, elements)

def create_string(text: str) -> Value:
    # Python string is immutable and gc'd
    return Value(STRING_TYPE, text)
```

## Implementation Files

```
thoxa/
├── types.py           # Type definitions and registry
├── type_checker.py    # Static type checking
├── type_inference.py  # Type inference engine
├── patterns.py        # Pattern matching
├── generics.py        # Generic type handling
└── runtime_types.py   # Runtime type information
```

## Future Enhancements

### Near Term
- User-defined generic types
- Type aliases
- Intersection types
- Gradual typing for migration

### Long Term
- Dependent types
- Higher-kinded types
- Type-level computation
- Effect types