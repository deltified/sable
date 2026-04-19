# Sable Compiler Progress

Date: 2026-04-19
Repository: sable

## Current Status

The compiler now supports two validated end-to-end flows on Windows 11:

1. Frontend/Sema/MIR + direct execution through a MIR runtime interpreter (`run` command).
2. LLVM IR emission (`ir` command) behind `llvm-backend` with LLVM at `C:\Program Files\LLVM`.

Verified commands:
- `cargo test`
- `cargo run -- run examples/run_showcase.sable`
- `$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"; cargo check --features llvm-backend`
- `$env:LLVM_SYS_221_PREFIX = "C:\Program Files\LLVM"; cargo run --features llvm-backend -- ir examples/struct_param_member.sable`

Observed `run` output for `examples/run_showcase.sable`:
- `Hello, Sable`
- `2`
- `10`
- `program returned: 10`

## Implemented So Far

### 1. Compiler Infrastructure
- Deterministic source loading with stable file IDs.
- Span tracking across lexer/parser/sema/MIR for diagnostics.
- Structured diagnostics with deterministic sort order.

### 2. Frontend (Lexer + Parser)
- Tokenization and parsing for core language forms: structs, functions, attributes, effects, control flow, member/index access, calls, ranges, and typed declarations.
- String and numeric literal support.

### 3. Semantic Analysis (Sema)
- Symbol/type/effect checks for baseline language.
- Loop-context diagnostics:
  - `SEM016`: `break` outside loop.
  - `SEM017`: `continue` outside loop.
- Struct field-order/index metadata for deterministic downstream lowering.
- Builtin member-call typing and effects for:
  - `io.out`
  - `str.concat`, `str.len`
  - `vec.new_i64`, `vec.push`, `vec.get`, `vec.len`
- String `+` typing support.
- Vector indexing typing support (`vec_i64[index]`).

### 4. MIR Lowering and Optimization
- CFG-based typed MIR lowering for expressions/control flow.
- `for` lowering over fixed-size arrays via index loops (`IndexLoad`).
- Struct metadata (`MirStruct`) propagated into MIR program.
- Builtin call resolution expanded for `io`, `str`, and `vec` runtime intrinsics.
- String constant folding for `+`, `==`, `!=`.
- Default local init for `str` now emits empty string constant.

### 5. Runtime Execution Path
- New MIR runtime interpreter module.
- New CLI command: `run <file.sable>`.
- Runtime builtins implemented:
  - `io.out`
  - `str.concat`, `str.len`
  - `vec.new_i64`, `vec.push`, `vec.get`, `vec.len`
- Runtime unit test added and passing.

### 6. LLVM Codegen (Windows)
- Backend remains optional behind `llvm-backend`.
- Windows path without `llvm-config` is supported via explicit configuration and linking.
- `IndexLoad` and `MemberLoad` lowering implemented.
- MIR struct metadata now drives named struct type lowering in LLVM backend.
- Verified IR generation on Windows with LLVM prefix configured.

### 7. CLI and Examples
- Commands now supported: `tokens`, `ast`, `check`, `mir`, `run`, `ir`.
- Added end-to-end execution example: `examples/run_showcase.sable`.
- Existing backend validation examples retained:
  - `examples/array_for.sable`
  - `examples/struct_param_member.sable`

### 8. Tests
- All current unit tests pass (6/6), including runtime strings/vectors test.

## Remaining Gaps (Major)

- Collections are still narrow: only `vec_i64` exists (no generic `vec<T>` yet).
- String library is still minimal (`concat`, `len`, `+` typing/folding only).
- LLVM backend does not yet lower full runtime-backed string/vector semantics.
- Aggregate store/reference-heavy codegen paths still partial.
- Borrow checker, region checker, and full determinism checker are not yet implemented.
- `try/catch` is still not lowered end-to-end.

## Logical Next Steps

1. Generalize vectors from `vec_i64` to `vec<T>` in type system + sema + MIR + runtime.
2. Expand string API (slice/substr, contains/find, comparisons, conversions) with effect/type rules and tests.
3. Implement integration tests for CLI `run` command using example programs.
4. Continue LLVM parity by lowering runtime-compatible string/vector operations to IR (or explicit runtime calls).
5. Complete aggregate stores and ref-based aggregate access in backend.

## Near-Term Milestone

Milestone M2: "Runnable baseline language"
- Goal: core programs with control flow, structs, strings, and vectors compile through sema/MIR and execute via `run` with deterministic behavior.
- Exit criteria:
  - stable `run` outputs for representative examples,
  - integration tests for `run`,
  - documented limits for features not yet codegen-backed in LLVM path.
