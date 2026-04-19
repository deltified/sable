# Sable Compiler Progress

Date: 2026-04-19
Repository: sable

## Current Status

The compiler now builds and runs on Windows 11 without requiring a local LLVM installation for frontend and MIR workflows.

LLVM-backed IR codegen is now also operational on Windows when the LLVM path is set.

Verified working commands:
- `cargo build`
- `cargo test`
- `cargo run -- check examples/basics.sable`
- `cargo run -- mir examples/basics.sable`
- `$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"; cargo run --features llvm-backend -- ir examples/basics.sable`
- `$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"; cargo run --features llvm-backend -- ir examples/array_for.sable`
- `$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"; cargo run --features llvm-backend -- ir examples/struct_param_member.sable`

LLVM IR emission is available behind an opt-in Cargo feature:
- `cargo run --features llvm-backend -- ir <file.sable>`

Without the feature, the `ir` command fails with a clear actionable message.

## Implemented So Far

### 1. Compiler Infrastructure
- Deterministic source loading with stable file IDs.
- Span tracking across lexer/parser/sema/MIR for diagnostics.
- Structured diagnostics framework with stable sort order.

### 2. Lexer
- Tokenization for keywords, punctuation, operators, attributes, and effects syntax.
- Numeric and string literal lexing.
- Comment and whitespace handling.
- Lex-time error reporting (invalid chars, unterminated strings).

### 3. Parser
- Top-level items: imports, structs, functions (including extern signatures).
- Attributes: `@name(...)` and key/value-style arguments.
- Effects clauses: `effects(...)` and `raise(...)` parsing.
- Statements: `let`, `return`, `raise`, `if/else`, `while`, `for`, `break`, `continue`, expression statements, nested blocks.
- Expressions: unary/binary ops, assignment and compound assignment, postfix increment, calls, member access, indexing, range operator `..`.
- Type syntax: named types, refs (`&T`, `ref T`, `ref<region> T`), arrays (`[T; N]`, `[T]`).

### 4. Semantic Analysis (Sema)
- Top-level symbol collection for structs/functions.
- Duplicate declaration diagnostics (structs, fields, functions, locals).
- Baseline type checking for numeric, bool, string, arrays, refs, named types.
- Call checking: arity/type matching and unknown callee diagnostics.
- Effect propagation and undeclared effect diagnostics.
- Deterministic restriction checks for `@deterministic` (`io`/`unsafe` disallowed).
- Member/index typing checks.
- Assignment target validation.

Newly added in this iteration:
- Loop-context diagnostics:
  - `SEM016`: `break` outside a loop.
  - `SEM017`: `continue` outside a loop.
- `for` iterable tightening for current backend compatibility:
  - supports ranges and fixed-size arrays.
  - explicit diagnostics for unsized arrays and strings in `for` until lowering/runtime support is added.
- Struct metadata now preserves deterministic field order and field-index maps for downstream MIR/codegen lowering.

### 5. MIR Lowering + Optimization
- Typed CFG-based MIR with explicit blocks and terminators.
- Lowering for control flow (`if/while/for`), expressions, calls, assignments, and loop control.
- Constant folding pass and dead-branch/dead-block elimination.

Newly added in this iteration:
- MIR lowering for `for` over fixed-size arrays.
- Array iteration lowers via index-based loop and emits `IndexLoad` per iteration.
- Refactor of for-loop lowering into reusable internal helpers.
- MIR program now carries struct metadata (`MirStruct`) so backend lowering has deterministic field indices.

### 6. LLVM Codegen
- Existing Inkwell-based backend retained.
- Now feature-gated as `llvm-backend` so platform setup is optional for frontend/MIR work.
- Clear runtime error if `ir` is requested without LLVM backend enabled.
- Windows `llvm-config` absence is handled by backend configuration changes plus explicit linking against `LLVM-C` from `LLVM_SYS_221_PREFIX`.
- Implemented `IndexLoad` lowering to LLVM GEP+load for fixed-size array iteration.
- Implemented `MemberLoad` lowering to LLVM struct-field GEP+load.
- Added named-struct type declaration/definition lowering from MIR metadata to LLVM named struct types.

### 7. CLI
- Commands supported: `tokens`, `ast`, `check`, `mir`, `ir`.
- `ir` command behavior is now explicit about feature requirements.

### 8. Tests
- Existing parser/sema/MIR tests preserved.
- Added sema regression test for invalid `break`/`continue` usage.
- Added MIR regression test proving fixed-size array `for` loops emit `IndexLoad`.
- Test suite currently passes (5/5).
- Added backend validation examples for array indexing and struct member access (`examples/array_for.sable`, `examples/struct_param_member.sable`) and confirmed IR emission on Windows.

## What Is Still Missing (Major)

- Full LLVM codegen coverage for:
  - string constants / string runtime representation
  - index/member store paths (writes through aggregate lvalues)
  - reference-based aggregate access (member/index through refs)
- Struct type lowering and field layout integration in backend.
- Borrow checker and region checker over CFG.
- Determinism checker depth beyond current effect-level constraints.
- `try/catch` semantics and lowering.
- Richer effect metadata usage in MIR optimization/codegen.
- Runtime layer (alloc/task/replay/hot-reload) beyond bootstrap scope.

## Logical Next Steps (Priority Order)

1. Complete backend parity for existing MIR:
- implement index/member store lowering and ref-based aggregate access in LLVM codegen.
- implement string constants and baseline string runtime interop in LLVM codegen.
- add IR golden tests for member/index-heavy programs.

2. Finish iterable parity across sema/MIR/codegen:
- either implement string iteration (`for x in str`) end-to-end or intentionally gate it everywhere with consistent diagnostics.
- support unsized-array iteration where semantics are defined.

3. Strengthen type-system guarantees:
- validate unresolved named types in declarations and local annotations.
- improve assignability/coercion rules and diagnostics quality.

4. Expand language surface already tokenized but partial:
- implement `try/catch` in AST/sema/MIR.
- add focused tests for error-flow and effect interactions (`raise(...)`).

5. Grow test coverage and CI quality gates:
- add integration tests for CLI commands (`check`, `mir`, feature-gated `ir`).
- add negative tests for unsupported forms to lock behavior.
- add deterministic ordering snapshots for diagnostics and MIR output.

## Suggested Near-Term Milestone

Milestone M1: "Backend parity for current MIR"
- Goal: any program accepted by `check` and `mir` in the baseline numeric/array/struct subset should also emit LLVM IR when built with `--features llvm-backend`.
- Exit criteria:
  - no `not implemented` backend errors for `MemberLoad`, `IndexLoad`, string constants in supported subset.
  - passing unit/integration tests for the new backend paths.
  - updated docs and examples demonstrating full path: source -> sema -> MIR -> LLVM IR.
