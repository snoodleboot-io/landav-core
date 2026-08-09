# Core Conventions Rust

Language:             rust
Runtime:              1.97
Package Manager:      Cargo
Linter:               ['clippy']
Formatter:           rustfmt

### Naming Conventions

Files:               snake_case
Variables:          snake_case
Constants:          UPPER_SNAKE
Classes/Types:      PascalCase
Functions:          snake_case
Database tables:    snake_case
Environment vars:   UPPER_SNAKE_CASE always

## Rust-Specific Rules

### Error Handling
- Use `Result<T, E>` for fallible operations - never panic in library code
- Use `?` operator for error propagation
- Use `thiserror` or `anyhow` for error handling
- Wrap errors with context using `map_err` or `with_context`

### Ownership & Borrowing
- Follow ownership rules - no use-after-free, no data races
- Use lifetimes when references must outlive their referents
- Prefer borrowing over cloning where possible
- Use `Arc` for shared ownership, `Rc` for single-threaded

### Traits & Generics
- Use traits for abstraction, not concrete types
- Prefer trait bounds over generic parameters
- Implement `Default`, `Clone`, `Debug`, `Display`, `Serialize`, `Deserialize` where appropriate

### Testing

#### Coverage Targets
Line:           80
Branch:           70
Function:           90
Statement:           85
Mutation:           80
Path:           60

#### Test Types

##### Unit Tests
- One function or method in isolation
- Mock all external dependencies
- Use framework-specific setup/teardown patterns

##### Integration Tests
- Test at service or module boundary
- Use real or in-memory implementations of external services
- Clean up test data after each test

#### Framework & Tools
Framework:         built-in