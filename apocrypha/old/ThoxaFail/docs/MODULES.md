# Thoxa Module System

## Overview

Thoxa uses a file-based module system where every `.thx` file is implicitly a module. Modules contain type definitions, functions, and values that can be imported and used in other files.

## Core Concepts

### File Modules

- **Every .thx file is a module**: No explicit module declaration needed
- **Named by filename**: `Math.thx` creates module `Math`
- **Import with paths**: Use relative or absolute paths

### Key Properties

- **Private by default**: All members are private unless marked `export`
- **Singleton instances**: Imported modules are singletons
- **Path-based resolution**: Import paths resolve to filesystem locations
- **Circular dependency detection**: Prevents circular module dependencies

## Module Files

### File Module Example (Math.thx)

```thoxa
# File: Math.thx
export let pi: Int64 = 314;  # Simplified pi * 100
export let e: Int64 = 271;   # Simplified e * 100

export fn add(a: Int64, b: Int64) -> Int64 {
    a + b
};

export fn multiply(x: Int64, y: Int64) -> Int64 {
    x * y
};
```

## Import System

### Import Syntax

```thoxa
# Default import - imports module as a type
import Math from "./Math";
let math: Math;  # Create instance
let result: Int64 = math.add(10, 20);

# Destructured import - imports specific members
import { add, multiply, pi } from "./Math";

# Use destructured imports directly  
let sum: Int64 = add(5, 3);
let product: Int64 = multiply(4, 7);

# Type and instance import
import Point, point from "./Point";
# Point is the type, point is a singleton instance
```

### Import Resolution

- Relative paths: `"./Module"`, `"../Module"`
- File extension optional: `"./Math"` → `"./Math.thx"`
- Standard library: `"../std/Io"` (relative to project root)

## Types in Modules

### Defining Types in Module Files

```thoxa
# File: Point.thx
export type Point {
    export let x: Int64 = 0;
    export let y: Int64 = 0;
    
    export fn distance(self: &Point) -> Int64 {
        self.x + self.y
    }
}

# Export a singleton instance
export let origin: Point = Point { x: 0, y: 0 };
```

### Name Resolution

When resolving names inside a function:
1. Check current module scope first
2. Then check function parameters and local variables  
3. Finally check imported symbols

This allows natural function composition while maintaining clear module boundaries.

## Examples

### Basic Module Usage


### Cross-module Dependencies

```thoxa
# File: Geometry.thx
import Math from "./Math";

export type Geometry {
    export fn circleArea(radius: Int64) -> Int64 {
        let math: Math;
        math.multiply(math.pi, math.multiply(radius, radius)) / 100
    }
}

# File: Main.thx  
import Geometry from "./Geometry";

let geo: Geometry;
let area: Int64 = geo.circleArea(10);  # Returns: 314
```

### Standard Library Import

```thoxa
# Import from standard library
import { printStr, printStrLn } from "../std/Io";

printStrLn("Hello from Thoxa!");
printStr("No newline");
```