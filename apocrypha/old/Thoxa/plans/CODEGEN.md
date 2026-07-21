# Code Generation Plan

## Overview

Remaining code generation work for Thoxa compiler.

## Key Concepts

1. **Values in Thoxa**: Everything is a 16-byte value (type pointer + data)
2. **QBE Types**: We'll primarily use `l` (long/64-bit) for pointers and integers
3. **Memory Layout**: Stack for values, heap for objects
4. **Reference Counting**: Heap objects need RC management


## Remaining Work

### Value Semantics
- [ ] Full 16-byte value structure handling
- [ ] Pack/unpack values for function calls
- [ ] Extract data fields when needed


### Runtime Initialization
- [ ] Set up runtime initialization in main

### Return Handling
- [ ] Jump to function epilogue for cleanup
- [ ] Handle early returns with cleanup

### Memory Management

#### Heap Allocation
- [ ] Implement allocator interface
- [ ] Add reference count field
- [ ] Handle allocation failure

#### Reference Counting
- [ ] Increment on copy
- [ ] Decrement on scope exit
- [ ] Free at zero count

#### String Literals
- [ ] Allocate string on heap
- [ ] Store pointer in value
- [ ] Set initial ref count

### Type System

#### Type Checking
- [ ] Verify operations are valid
- [ ] Check function arguments
- [ ] Ensure assignments are compatible

#### Type Information
- [ ] Generate runtime type descriptors
- [ ] Store type size and name
- [ ] Support for future introspection

#### Generic Types
- [ ] Full generic instantiation for May<T> and Trial<T,E>
- [ ] User-defined generic types

### Pattern Matching
- [ ] Generate branch table for general patterns
- [ ] Handle pattern conditions
- [ ] Extract values from constructors
- [ ] Pattern matching on user-defined types

### Try Operator
- [ ] Check for Fail constructor
- [ ] Early return on failure
- [ ] Propagate error value

### Module System

#### Module Compilation
- [ ] Compile each module separately
- [ ] Generate symbol exports
- [ ] Handle module dependencies

#### Linking
- [ ] Resolve cross-module references
- [ ] Generate final executable
- [ ] Handle external functions (print, etc.)

### String Methods (Immediate Priority)

#### Method Exposure
- [ ] Expose thoxa_string_concat as .concat() method
- [ ] Expose thoxa_string_length as .length() method  
- [ ] Expose thoxa_string_substring as .substring() method
- [ ] Expose thoxa_string_find as .indexOf() method
- [ ] Expose thoxa_string_char_at as .charAt() method
- [ ] Add .contains() method using indexOf


#### String Formatting
- [ ] Format specifiers for interpolation
- [ ] toString() for user-defined types

### Collection Support

#### Array Enhancements
- [ ] Array.map() method
- [ ] Array.filter() method
- [ ] Array.reduce() method
- [ ] Array.isEmpty() method
- [ ] Array.first() and last() methods

#### Dynamic Arrays (Vec<T>)
- [ ] Growable arrays with capacity management
- [ ] Similar API to arrays
- [ ] Efficient reallocation strategy

