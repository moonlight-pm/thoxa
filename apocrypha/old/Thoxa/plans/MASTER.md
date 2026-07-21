# Thoxa Development Master Plan

## Project Status: Pre-Alpha

See PROGRESS.md for completed features.

## Remaining Work

### Immediate Priorities

#### Parser Improvements
- [ ] Try operator (?) - parsed but needs code generation
- [ ] Module system (import/export) - parsed but needs implementation

#### String Methods
- [ ] Expose string methods to Thoxa code
  - [ ] .length() method
  - [ ] .substring(start, end) method
  - [ ] .indexOf(needle) method
  - [ ] .charAt(index) method
  - [ ] .contains(needle) method

#### Pattern Matching
- [ ] Pattern matching on user-defined types
- [ ] Union type patterns
- [ ] Destructuring patterns

#### Collections
- [ ] Vec<T> - Dynamic arrays with growth
- [ ] HashMap<K,V> - Hash tables
- [ ] Set<T> - Hash sets

### Type System & Abstractions

#### Generics
- [ ] User-defined generic types
- [ ] Generic functions
- [ ] Type constraints/bounds
- [ ] Better type parameter inference
- [ ] Make May<T> and Trial<T,E> truly generic (currently monomorphized)

#### Interfaces/Traits
- [ ] Interface definitions
- [ ] Implementation blocks
- [ ] Trait bounds on generics
- [ ] Static dispatch
- [ ] Dynamic dispatch (future)

### Module System
- [ ] Multi-file compilation
- [ ] Module resolution and imports
- [ ] Visibility enforcement (export keyword)
- [ ] Package management (future)

### I/O & System
- [ ] File I/O operations
- [ ] Path manipulation
- [ ] Directory operations
- [ ] Command-line arguments
- [ ] Environment variables

### Developer Experience
- [ ] Language Server Protocol (LSP)
- [ ] Build system improvements
- [ ] Testing framework
- [ ] Documentation generation
- [ ] Better error messages

### Optimizations
- [ ] Dead code elimination
- [ ] Constant folding
- [ ] Function inlining
- [ ] Tail call optimization
- [ ] Small string optimization

