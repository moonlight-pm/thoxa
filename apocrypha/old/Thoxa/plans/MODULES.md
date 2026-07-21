# Module System in Thoxa

## Overview

Modules in Thoxa are conceptually units that are instantiated once (singletons). A module file is implicitly a type definition that gets instantiated when imported.

## Module Structure

A module file contains the same elements as a type body:

```thoxa
# math.thx - A module file
export let PI = 3.14159
export let E = 2.71828

let TAU = PI * 2  # Private constant

# Module initialization code
println("Math module loaded")

export fn sqrt(x!: Float) -> Float {
    # Implementation
}

export fn pow(base!: Float, exp!: Float) -> Float {
    # Implementation
}

# Private helper
fn validate(x: Float) -> Bool {
    x >= 0 && isFinite(x)
}

# Nested type
export type Complex {
    export let real: Float
    export let imag: Float
}
```

## Import Semantics

When a module is imported, it is instantiated exactly once:

```thoxa
import math  # Creates singleton: let math = Module$math$hash()

# Selective imports still instantiate the whole module
import { sqrt, PI } from "math"  
# Equivalent to:
# let _math = Module$math$hash()
# let sqrt = _math.sqrt
# let PI = _math.PI
```

## Module Initialization

Module initialization code runs **once** when first imported:

```thoxa
# logger.thx
export let mut logCount = 0

# This runs once when module is first imported
let logFile = openFile("app.log")
println("Logger initialized")

export fn log(message: String) {
    logCount += 1
    writeFile(logFile, "{logCount}: {message}")
}
```

## Circular Dependencies

Circular imports are detected and handled:

```thoxa
# a.thx
import b
export let x = 10

# b.thx  
import a
export let y = a.x + 5  # Must handle carefully
```

The module system ensures each module is only instantiated once, even in circular cases.

## Module Names

Modules receive anonymous internal type names like `Module$hash`. The import binding name is what matters to users:

```thoxa
import utils from "./helpers/utils"  # 'utils' is the local name
import { format } from "../formatters/text"  # Module type name is anonymous
```

## Comparison with Types

| Aspect | Module | Type |
|--------|--------|------|
| Instantiation | Once (singleton) | Multiple times |
| Name | Anonymous internal | User-defined |
| Import | Creates/accesses singleton | Just makes available |
| Parameters | None | Constructor parameters |
| File | One per file | Multiple per file |

## Implementation Note

Internally, a module file is transformed into:

```thoxa
# math.thx becomes:
type Module$math$7f3a {
    export let PI = 3.14159
    # ... rest of module
}

# Import creates singleton:
let math = Module$math$7f3a()  # Called once
```

## Future Considerations

- **Lazy Loading**: Defer module initialization until first use
- **Hot Reloading**: Reload modules during development
- **Module Resolution**: Package management and versioning
- **Export Aliases**: `export { sqrt as squareRoot }`