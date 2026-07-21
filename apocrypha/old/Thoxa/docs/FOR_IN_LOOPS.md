# For-In Loop Implementation Plan

## Overview
For-in loops provide iteration over arrays (and potentially other collections).

## Syntax
```thoxa
for element in array {
    // body using element
}

// With index
for (element, index) in array.enumerate() {
    // body using element and index  
}
```

## Implementation Steps

### 1. AST Extension
Add to `Expr` enum:
```rust
ForIn {
    variable: String,
    iterable: Box<Expr>,
    body: Block,
    span: Span,
}
```

### 2. Parser Support
- Add `for` keyword to lexer
- Parse `for <ident> in <expr> { <block> }`
- Initially support only simple identifier binding

### 3. Code Generation
Transform for-in loop into while loop:
```thoxa
// Source:
for elem in arr {
    println(elem);
}

// Transforms to:
{
    let _iter_arr = arr;
    mut _index = 0;
    while _index < _iter_arr.count {
        let elem = _iter_arr[_index];
        println(elem);
        _index = _index + 1;
    }
}
```

### 4. Optimizations (Future)
- Iterator protocol for custom types
- Reverse iteration
- Step/stride support
- Early exit optimization

## Testing
```thoxa
fn test_for_in() {
    let nums = [1, 2, 3, 4, 5];
    mut sum = 0;
    
    for n in nums {
        sum = sum + n;
    }
    
    assert(sum == 15);
}
```

## Phases

### Phase 1: Basic For-In (Now)
- Simple identifier binding
- Arrays only
- Forward iteration

### Phase 2: Extended Features (Later)
- Destructuring: `for (elem, idx) in arr.enumerate()`
- Ranges: `for i in 0..10`
- Custom iterators