# Printing API Plan

## Overview

This document outlines the remaining work for completing Thoxa's printing API. The core infrastructure is already implemented:
- ✅ Template literals with `${}` interpolation
- ✅ Type conversion system with `as` keyword
- ✅ Array types with stack-based allocation
- ✅ Variadic function parsing and code generation
- ✅ Basic print functions in std/IoVariadic.thx

## Current Status

### Completed
- ✅ Any type added to type system
- ✅ Type checking allows Any to accept any value
- ✅ Basic infrastructure for polymorphic types

### Current Limitations

The main limitation is that Any type lacks runtime type information, so we cannot:
1. Check what type an Any value contains at runtime
2. Convert Any values to strings dynamically
3. Implement truly polymorphic print functions

Example of what works vs what doesn't:
```thoxa
# What works - type system accepts Any
let x: Any = 42;
let y: Any = "Hello";

# What doesn't work yet - runtime operations on Any
# print(x);  // Can't convert Any to string at runtime

# Current workaround (limited to Int64)
export fn println(...args: Int64) -> Void {
    let i = 0;
    loop {
        if i >= args.length() { break; };
        @print(args[i]);
        if i < args.length() - 1 { @printStr(" "); };
        i = i + 1;
    };
    @printStr("\n");
    0
}
```

## Immediate Tasks

### 1. ~~Create Any Type~~ ✅ DONE
Basic Any type exists in the type system.

### 2. Implement Runtime Type Information
Following the design in [RUNTIME_TYPE_INFO.md](RUNTIME_TYPE_INFO.md):
```thoxa
# Goal: Enable this
export fn print(...args: Any) -> Void {
    for arg in args {
        # With new as? operator
        match arg as? String {
            Ok(s) => @printStr(s),
            Err(_) => @printStr("[unprintable]")
        }
    }
}
```

### 2. Implement Spread Operator
Enable spreading arrays into function calls:
```thoxa
let values = [1, 2, 3];
println(...values);  # Should print: 1 2 3
```

### 3. Update Examples
Replace all uses of `@print` and `@printStr` with new print functions:
```thoxa
# Old
@printStr("Hello ");
@print(42);
@printStr("\n");

# New
println("Hello", 42);
```

### 4. Deprecate Old Builtins
Mark `@print` and `@printStr` as deprecated with migration messages.

## Future Enhancements

### User-Defined Conversions
Generate actual function calls for user-defined `as String` conversions:
```thoxa
type Point {
    export let x: Int64 = 0;
    export let y: Int64 = 0;
    
    as String {
        "(${x}, ${y})"  # This should compile to actual code
    }
}
```

### String Builder Optimization
Optimize template literals to minimize allocations:
```thoxa
# Currently: Multiple allocations
"Hello ${name}, you scored ${score}%!"

# Future: Single allocation with pre-calculated size
```

### Error Handling
Better error messages for conversion failures:
```thoxa
# Instead of crash, handle gracefully
if value can String {
    print(value as String);
} else {
    print("[${typeof(value)}]");  # Show type name
}
```

## Design Decisions

1. **No Duck Typing**: Using explicit type conversions (`as`) instead
2. **Stack Arrays**: Variadic arguments use stack-allocated arrays
3. **Space Separator**: Print functions separate arguments with spaces
4. **Auto Flush**: All print functions flush output immediately
5. **Template Literals**: Use regular strings with `${}`, not backticks

## Implementation Priority

1. **Any type** - Unblocks polymorphic printing
2. **Spread operator** - Natural array printing
3. **Example updates** - Modernize codebase
4. **Deprecation** - Guide users to new API

This order delivers maximum value while maintaining backward compatibility.