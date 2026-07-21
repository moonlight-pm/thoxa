# Error Handling Patterns - Result/Either Monads

## Overview

Result/Either monads provide a type-safe way to handle errors without exceptions. They encode success and failure as explicit return types, forcing callers to handle both cases.

## Language Implementations

### 1. **Rust - Result<T, E>**

Rust's Result is the gold standard for error handling monads.

```rust
// Definition
enum Result<T, E> {
    Ok(T),
    Err(E),
}

// Usage
fn divide(x: f64, y: f64) -> Result<f64, String> {
    if y == 0.0 {
        Err("Division by zero".to_string())
    } else {
        Ok(x / y)
    }
}

// Pattern matching
match divide(10.0, 2.0) {
    Ok(result) => println!("Result: {}", result),
    Err(e) => println!("Error: {}", e),
}

// Combinators
let result = divide(10.0, 2.0)
    .map(|x| x * 2.0)           // Transform success value
    .and_then(|x| divide(x, 5.0)) // Chain operations
    .map_err(|e| format!("Failed: {}", e)); // Transform error

// The ? operator for early return
fn calculate() -> Result<f64, String> {
    let x = divide(10.0, 2.0)?;  // Returns early if Err
    let y = divide(x, 5.0)?;
    Ok(y * 2.0)
}
```

### 2. **Haskell - Either a b**

Haskell uses Either, where Left is error and Right is success.

```haskell
-- Definition
data Either a b = Left a | Right b

-- Usage
divide :: Double -> Double -> Either String Double
divide x 0 = Left "Division by zero"
divide x y = Right (x / y)

-- Pattern matching
case divide 10 2 of
    Left err -> putStrLn $ "Error: " ++ err
    Right result -> putStrLn $ "Result: " ++ show result

-- Monadic operations
import Control.Monad

calculate :: Either String Double
calculate = do
    x <- divide 10 2    -- Automatically returns Left if error
    y <- divide x 5
    return (y * 2)

-- Functor/Applicative operations
result = (* 2) <$> divide 10 2          -- fmap
result = divide 10 2 >>= \x -> divide x 5  -- bind
```

### 3. **Swift - Result<Success, Failure>**

Swift adopted Result in version 5.

```swift
// Built-in Result type
enum Result<Success, Failure: Error> {
    case success(Success)
    case failure(Failure)
}

// Usage
enum MathError: Error {
    case divisionByZero
}

func divide(_ x: Double, by y: Double) -> Result<Double, MathError> {
    guard y != 0 else {
        return .failure(.divisionByZero)
    }
    return .success(x / y)
}

// Pattern matching
switch divide(10, by: 2) {
case .success(let result):
    print("Result: \(result)")
case .failure(let error):
    print("Error: \(error)")
}

// Functional operations
let result = divide(10, by: 2)
    .map { $0 * 2 }
    .flatMap { divide($0, by: 5) }

// Getting value with try
let value = try divide(10, by: 2).get()
```

### 4. **Kotlin - Result<T>**

Kotlin has a built-in Result type since 1.3.

```kotlin
// Usage (Result is built-in)
fun divide(x: Double, y: Double): Result<Double> {
    return if (y == 0.0) {
        Result.failure(IllegalArgumentException("Division by zero"))
    } else {
        Result.success(x / y)
    }
}

// Handling
val result = divide(10.0, 2.0)
    .map { it * 2 }
    .mapCatching { divide(it, 5.0).getOrThrow() }
    .onSuccess { println("Result: $it") }
    .onFailure { println("Error: ${it.message}") }

// Getting value
val value = result.getOrNull()
val valueOrDefault = result.getOrDefault(0.0)
```

### 5. **F# - Result<'T, 'Error>**

F# has built-in Result type with comprehensive support.

```fsharp
// Usage
let divide x y =
    if y = 0.0 then
        Error "Division by zero"
    else
        Ok (x / y)

// Pattern matching
match divide 10.0 2.0 with
| Ok result -> printfn "Result: %f" result
| Error msg -> printfn "Error: %s" msg

// Computation expression
let calculate = result {
    let! x = divide 10.0 2.0
    let! y = divide x 5.0
    return y * 2.0
}

// Railway-oriented programming
let (>>=) result f =
    match result with
    | Ok value -> f value
    | Error e -> Error e

let final = 
    divide 10.0 2.0
    >>= fun x -> divide x 5.0
    >>= fun y -> Ok (y * 2.0)
```

### 6. **OCaml - Result.t**

OCaml has a polymorphic result type.

```ocaml
(* Built-in since 4.08 *)
type ('a, 'e) result = 
  | Ok of 'a
  | Error of 'e

(* Usage *)
let divide x y =
  if y = 0.0 then
    Error "Division by zero"
  else
    Ok (x /. y)

(* Pattern matching *)
match divide 10.0 2.0 with
| Ok result -> Printf.printf "Result: %f\n" result
| Error msg -> Printf.printf "Error: %s\n" msg

(* Monadic bind *)
let ( >>= ) r f =
  match r with
  | Ok v -> f v
  | Error _ as e -> e

let calculate =
  divide 10.0 2.0 >>= fun x ->
  divide x 5.0 >>= fun y ->
  Ok (y *. 2.0)
```

### 7. **TypeScript - Custom Result Type**

TypeScript doesn't have built-in Result, but it's commonly implemented.

```typescript
// Custom implementation
type Result<T, E> = 
  | { ok: true; value: T }
  | { ok: false; error: E };

const Ok = <T>(value: T): Result<T, never> => 
  ({ ok: true, value });

const Err = <E>(error: E): Result<never, E> => 
  ({ ok: false, error });

// Usage
function divide(x: number, y: number): Result<number, string> {
  if (y === 0) {
    return Err("Division by zero");
  }
  return Ok(x / y);
}

// Pattern matching with discriminated unions
const result = divide(10, 2);
if (result.ok) {
  console.log(`Result: ${result.value}`);
} else {
  console.log(`Error: ${result.error}`);
}

// Functional helpers
function map<T, U, E>(
  result: Result<T, E>, 
  f: (value: T) => U
): Result<U, E> {
  return result.ok ? Ok(f(result.value)) : result;
}

function andThen<T, U, E>(
  result: Result<T, E>,
  f: (value: T) => Result<U, E>
): Result<U, E> {
  return result.ok ? f(result.value) : result;
}
```

### 8. **Scala - Either[L, R] and Try[T]**

Scala provides multiple error handling types.

```scala
// Using Either
def divide(x: Double, y: Double): Either[String, Double] = {
  if (y == 0) Left("Division by zero")
  else Right(x / y)
}

// Pattern matching
divide(10, 2) match {
  case Right(result) => println(s"Result: $result")
  case Left(error) => println(s"Error: $error")
}

// For comprehension
val result = for {
  x <- divide(10, 2)
  y <- divide(x, 5)
} yield y * 2

// Using Try for exception handling
import scala.util.{Try, Success, Failure}

def safeDivide(x: Double, y: Double): Try[Double] = Try(x / y)

safeDivide(10, 0) match {
  case Success(value) => println(s"Result: $value")
  case Failure(exception) => println(s"Error: ${exception.getMessage}")
}
```

## Design Considerations for Thoxa

### 1. **Syntax Options**

```thoxa
# Option A: Rust-like
type Result[T, E] {
    Ok(T) | Err(E)
}

fn divide(x: Int64, y: Int64) -> Result[Int64, String] {
    if y == 0 {
        Result.Err("Division by zero")
    } else {
        Result.Ok(x / y)
    }
}

# Option B: ML-style
type Result a b = Ok a | Err b

fn divide(x: Int64, y: Int64) -> Result Int64 String {
    match y {
        0 => Err "Division by zero"
        _ => Ok (x / y)
    }
}

# Option C: Built-in with special syntax
fn divide(x: Int64, y: Int64) -> Int64 ! String {
    if y == 0 {
        throw "Division by zero"  # or 'fail' keyword
    };
    x / y  # implicit Ok
}
```

### 2. **Error Propagation**

```thoxa
# Option A: Explicit propagation
fn calculate() -> Result[Int64, String] {
    match divide(10, 2) {
        Ok(x) => match divide(x, 5) {
            Ok(y) => Ok(y * 2)
            Err(e) => Err(e)
        }
        Err(e) => Err(e)
    }
}

# Option B: Question mark operator (like Rust)
fn calculate() -> Result[Int64, String] {
    let x = divide(10, 2)?;
    let y = divide(x, 5)?;
    Ok(y * 2)
}

# Option C: Monadic do notation
fn calculate() -> Result[Int64, String] {
    do {
        x <- divide(10, 2);
        y <- divide(x, 5);
        return y * 2
    }
}
```

### 3. **Pattern Matching**

```thoxa
# Exhaustive matching required
let result = divide(10, 2);
match result {
    Ok(value) => @print(value),
    Err(msg) => @printStr(msg)
}

# With guards
match divide(x, y) {
    Ok(n) if n > 0 => @printStr("Positive"),
    Ok(n) => @printStr("Non-positive"),
    Err(e) => @printStr(e)
}
```

### 4. **Combinators**

```thoxa
# Essential combinators
impl Result[T, E] {
    fn map[U](self, f: T -> U) -> Result[U, E] { ... }
    fn mapErr[F](self, f: E -> F) -> Result[T, F] { ... }
    fn andThen[U](self, f: T -> Result[U, E]) -> Result[U, E] { ... }
    fn orElse[F](self, f: E -> Result[T, F]) -> Result[T, F] { ... }
    fn unwrapOr(self, default: T) -> T { ... }
}
```

## Recommendations for Thoxa

1. **Start Simple**: Begin with a basic `Result[T, E]` type and pattern matching
2. **Explicit Over Implicit**: Require explicit Ok/Err construction initially
3. **Exhaustive Matching**: Enforce exhaustive pattern matching for Result types
4. **Sugar Later**: Add syntactic sugar (like `?` operator) after core is stable
5. **Type Inference**: Ensure good type inference for Result chains
6. **Standard Errors**: Define a standard error type/trait for common cases
7. **Interop**: Consider how Result interacts with existing error handling (if any)

## Example Implementation Path

```thoxa
# Phase 1: Basic Result type
type Result[T, E] {
    export variant Ok(value: T);
    export variant Err(error: E);
}

# Phase 2: Pattern matching
fn processFile(path: String) -> Result[String, String] {
    let file = openFile(path);
    match file {
        Ok(f) => Ok(readContents(f)),
        Err(e) => Err("Failed to open: " + e)
    }
}

# Phase 3: Combinators
let result = openFile("data.txt")
    .map(|f| readContents(f))
    .mapErr(|e| "IO Error: " + e);

# Phase 4: Error propagation sugar
fn process() -> Result[Data, Error] {
    let file = openFile("data.txt")?;
    let contents = readContents(file)?;
    let data = parseData(contents)?;
    Ok(transform(data))
}
```

This progression allows for gradual complexity while maintaining backward compatibility.