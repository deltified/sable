# Sable Implementation Plan
## Version
- Target spec: Sable Language Specification v2.0 (Revised Alpha++)
- Backend target: LLVM
- Primary non-negotiables: safety, explicit effects, deterministic performance
- This document is a full implementation blueprint with determinism-aware decisions from day one

## Reading Guide
- Category 1: frontend and baseline language implementation
- Category 2: safety and semantic enforcement systems
- Category 3: advanced runtime and low-level systems
- Every work package includes implementation mechanics and guarantee alignment
- Every phase includes test strategy and acceptance gates

## Bootstrap Progress Snapshot (2026-04-19)
- C1-WP01 through C1-WP14: mostly implemented in bootstrap compiler form (lexer/parser/name+type+effect checks/MIR/CLI).
- C1-WP11 (structs): implemented with deterministic field ordering metadata propagated through MIR and LLVM lowering.
- C1-WP12 (control flow): implemented for if/while/for/break/continue with loop-context diagnostics and CFG lowering.
- C1-WP15 (vector baseline): partially implemented as `vec_i64` runtime/builtin path (`new_i64`, `push`, `get`, `len`) with MIR execution support.
- C1-WP21 and C1-WP22: implemented as typed CFG MIR with canonical lowering and baseline optimization passes.
- LLVM backend status: working on Windows for supported subset behind `llvm-backend`, including `IndexLoad` and `MemberLoad`.
- New bootstrap execution path: MIR interpreter (`run` command) supports end-to-end execution for current subset.
- Immediate focus for next milestone:
	- generalize `vec_i64` to generic vectors,
	- broaden string operations beyond concat/len,
	- improve LLVM parity for runtime-backed features.

## Global Program Architecture
- Compiler executable is split into frontend, middle-end, backend, and tooling layers
- Runtime library is split into memory runtime, task runtime, deterministic runtime, and hot reload runtime
- Standard library is split into pure core, effectful adapters, and optional platform bindings
- Tooling includes formatter, language server hooks, test harness, and conformance runner
- Determinism constraints are represented in IR metadata, not just parser annotations
- Effects are represented in function signatures and call graph summaries
- Borrow and region checking are represented as constraints solved over CFG-based lifetimes
- Layout semantics are represented in type descriptors and lowering rules
- Unsafe verification is represented with instrumentation toggles in the backend
- Reproducibility rules are represented in both compile flags and codegen behavior

## Compiler Pass Order (Top Level)
- Parse source to CST with complete span data
- Lower CST to AST and attach parsed attributes
- Build symbol tables and module graph
- Resolve names and imports
- Build type graph and infer local type variables
- Normalize function signatures including effect sets and error effects
- Build CFG and perform dataflow prepasses
- Run borrow checker and region checker
- Run effect checker and raise-effect checker
- Run deterministic eligibility checker
- Run attribute validation checker
- Run compile-time execution evaluator for @comp
- Lower to typed MIR with explicit control flow and effects
- Run MIR optimizations constrained by effect and determinism metadata
- Lower MIR to LLVM IR with target ABI contracts
- Emit objects, debug metadata, patch metadata, and optional replay metadata

## Determinism-Forward Rules Applied From Phase Zero
- Do not use unordered host hash maps in compiler analyses that affect emitted ordering
- Use stable IDs for symbols, types, blocks, and diagnostics
- Sort all collections before emission where ordering can impact output bits
- Keep diagnostic ordering deterministic by file ID and span start
- Canonicalize floating constant formatting at parse and IR emission time
- Canonicalize attribute argument order during AST normalization
- Use deterministic symbol mangling with explicit version salt
- Ensure generic instantiation order is canonical and not hash-table dependent
- Ensure metadata section ordering in object files is canonical
- Ensure replay hashes include only semantic fields and normalized target fields
- Ensure optimizer pass pipeline is fixed unless explicit non-deterministic mode is selected
- Ensure runtime task join order serialization is deterministic when required

## Repository and Module Layout
- compiler/src/driver for CLI and orchestration
- compiler/src/lexer for tokenization and trivia management
- compiler/src/parser for grammar and AST builder
- compiler/src/ast for syntax tree types and attribute nodes
- compiler/src/sema for name, type, effects, borrow, determinism checks
- compiler/src/mir for typed mid-level IR
- compiler/src/codegen for LLVM lowering and debug info
- compiler/src/verify for optional runtime verification instrumentation plans
- runtime/src/memory for region and allocator runtimes
- runtime/src/tasks for structured concurrency runtime
- runtime/src/replay for deterministic record and replay runtime
- runtime/src/hotreload for function table and patching runtime
- std/core for pure and deterministic APIs
- std/io for explicit io APIs with effects
- std/ffi for foreign wrappers and verification adapters
- tests/conformance for spec-level tests
- tests/replay for deterministic replay tests
- tests/hotreload for reload safety tests

## Cross-Cutting Acceptance Standards
- Every checker must have positive and negative tests
- Every diagnostics message must include error code and fix hint when possible
- Every guarantee from spec must map to one or more test suites
- Every deterministic guarantee must have bitwise reproducibility tests
- Every runtime safety guarantee must have sanitizer-backed stress tests
- Every attribute must have parser, sema, MIR, and codegen tests if relevant
- Every new feature must include docs and examples in examples/ directory

# Category 1: Basics
## Scope
- Implement lexer, parser, semantic basics, and codegen for baseline language
- Cover control flow, data structures, built-in types, vectors, maps, unordered maps, structs, refs, and raw pointers syntax and base lowering
- Keep all design choices compatible with future determinism mode

## C1-WP01 Source manager and file identity
- Objective: Build deterministic source loading and stable file IDs
- Implementation: Introduce source database with normalized absolute path keys
- Implementation: Assign sequential file IDs by canonical module graph traversal
- Implementation: Store line start indices for fast line-column mapping
- Guarantee alignment: Deterministic diagnostics and deterministic module ordering
- Validation: Snapshot test file ID ordering across repeated builds
- Validation: Fuzz path normalization including symlinks and relative imports
- Exit criteria: Stable file identity on all supported host filesystems

## C1-WP02 Token definitions and lexical grammar
- Objective: Define all tokens including attributes, effects syntax, and type syntax
- Implementation: Enumerate keywords, operators, delimiters, literals, and contextual tokens
- Implementation: Reserve future tokens for determinism and replay directives
- Implementation: Mark tokens with precedence category metadata for parser speed
- Guarantee alignment: No ambiguous tokenization for core grammar
- Validation: Golden token stream tests for representative source files
- Validation: Differential tests versus hand-authored expected token sequences
- Exit criteria: Lexer produces identical streams for identical input bytes

## C1-WP03 Lexer architecture and trivia model
- Objective: Implement streaming lexer with full span and trivia capture
- Implementation: Use byte index cursor and UTF-8 validation with explicit error tokens
- Implementation: Capture comments and whitespace as trivia attached to tokens
- Implementation: Store raw literal text for precise later numeric parsing
- Guarantee alignment: Deterministic formatting and exact diagnostics spans
- Validation: Property tests for span monotonicity and trivia attachment
- Validation: Invalid UTF-8 tests produce deterministic diagnostics
- Exit criteria: Lexer reaches linear complexity and stable output ordering

## C1-WP04 Numeric literal parsing baseline
- Objective: Parse integers and floats with exactness tracking
- Implementation: Support decimal, hex, binary integer forms with separators
- Implementation: Parse float forms with normalized exponent representation
- Implementation: Record literal overflow and precision warnings in token metadata
- Guarantee alignment: Future deterministic FP mode has canonical literal source
- Validation: Exhaustive boundary tests for i32, i64, u32, u64, f32, f64
- Validation: Cross-target tests for identical parsed literal canonical forms
- Exit criteria: Literal parser deterministic and target-independent at AST level

## C1-WP05 Parser framework and recovery strategy
- Objective: Build recursive-descent parser with robust error recovery
- Implementation: Implement Pratt expression parser with precedence table
- Implementation: Implement statement and declaration parsers with synchronization sets
- Implementation: Preserve parsed-but-invalid nodes for better diagnostics
- Guarantee alignment: Compiler provides precise compile-time violations
- Validation: Recovery tests ensure multiple errors can be reported per file
- Validation: Fuzz parser for panic-free behavior
- Exit criteria: Parser never crashes on malformed input

## C1-WP06 AST schema and node ownership
- Objective: Define AST nodes for modules, functions, structs, attributes, and expressions
- Implementation: Use arena allocation with stable node IDs for deterministic traversal
- Implementation: Attach source spans and attribute lists on all declaration nodes
- Implementation: Store unresolved type and effect syntax trees in declarations
- Guarantee alignment: Effect sets and attribute rules can be validated later
- Validation: AST serialization snapshot tests
- Validation: Deterministic traversal order tests
- Exit criteria: Stable AST representation for all baseline constructs

## C1-WP07 Module graph and import resolution
- Objective: Resolve modules and imports with deterministic order
- Implementation: Build directed acyclic import graph with cycle diagnostics
- Implementation: Resolve modules by explicit root and canonical path rules
- Implementation: Ensure import expansion order is lexical and stable
- Guarantee alignment: Deterministic build graph and reproducible errors
- Validation: Import cycle tests and duplicate module tests
- Validation: Rebuild determinism tests with shuffled filesystem order
- Exit criteria: Module graph stable and cycle-safe

## C1-WP08 Name binding and symbol table core
- Objective: Bind declarations and references in module and block scopes
- Implementation: Create nested scope frames with stable insertion indices
- Implementation: Resolve identifiers with shadowing diagnostics
- Implementation: Record symbol kinds and declaration spans for tooling
- Guarantee alignment: Predictable semantics and deterministic resolution
- Validation: Shadowing and unresolved symbol test matrix
- Validation: Stable symbol ID allocation tests
- Exit criteria: Name resolution complete for baseline language

## C1-WP09 Baseline type system foundations
- Objective: Implement primitive, composite, function, pointer, and reference type nodes
- Implementation: Add canonical interning table for structurally equal types
- Implementation: Implement type equality and assignability rules
- Implementation: Reserve hooks for region-annotated refs and effects-in-function-types
- Guarantee alignment: Type safety and future borrow/effect integration
- Validation: Unit tests for type formation and subtype checks
- Validation: Negative tests for illegal assignments and coercions
- Exit criteria: Typed AST generated for baseline expressions and statements

## C1-WP10 Built-in primitive types
- Objective: Implement bool, integer widths, float widths, and string type baseline
- Implementation: Define literal typing defaults and explicit cast rules
- Implementation: Provide deterministic overflow diagnostics policy
- Implementation: Define target-specific ABI sizes only in backend layer
- Guarantee alignment: Type safety and deterministic compile behavior
- Validation: Literal typing tests and cast legality tests
- Validation: Cross-target semantic equivalence tests at MIR level
- Exit criteria: Primitive typing stable and verified

## C1-WP11 Struct declarations and field access
- Objective: Implement plain struct declaration, initialization, and field access
- Implementation: Build struct type layout descriptor placeholders for later layout attributes
- Implementation: Validate unique field names and visibility rules
- Implementation: Lower field access to typed member operations in MIR
- Guarantee alignment: Zero-cost abstraction and layout-aware future extension
- Validation: Struct init, update, and nested field access tests
- Validation: Invalid field and missing field diagnostics tests
- Exit criteria: Struct semantics fully operational in baseline mode

## C1-WP12 Control flow statements
- Objective: Implement if, else, while, for, break, continue, and return
- Implementation: Build CFG edges during MIR lowering for each control flow construct
- Implementation: Ensure unreachable code diagnostics are deterministic and stable
- Implementation: Normalize loop constructs to canonical MIR forms
- Guarantee alignment: Predictable control flow lowering and deterministic diagnostics
- Validation: CFG shape snapshot tests per construct
- Validation: Loop edge-case tests with nested breaks and continues
- Exit criteria: Control flow constructs correctly type-checked and lowered

## C1-WP13 Expression semantics baseline
- Objective: Implement unary, binary, call, indexing, and member expressions
- Implementation: Apply operator precedence and associativity from parser tables
- Implementation: Resolve operator overloading rules if language permits via traits later
- Implementation: Lower call expressions with explicit callee and argument order
- Guarantee alignment: Deterministic evaluation order and predictable lowering
- Validation: Expression typing test suite with mixed precedence
- Validation: Call argument evaluation order tests
- Exit criteria: Expression system stable and deterministic

## C1-WP14 Function declarations and calls
- Objective: Implement function signatures, parameter binding, and call resolution
- Implementation: Parse and store effect clauses in signature nodes even before enforcement
- Implementation: Include function type with effect-set field in type interner
- Implementation: Lower calls preserving function signature metadata
- Guarantee alignment: Effects are part of type system from earliest phase
- Validation: Function type identity tests including effect annotations
- Validation: Mismatch diagnostics for call arity and types
- Exit criteria: Function calls fully typed and lowered

## C1-WP15 Baseline collections: vector type
- Objective: Implement vector syntax, typing, and baseline runtime ABI
- Implementation: Define vec<T> as stdlib-backed generic type with explicit methods
- Implementation: Mark mutating operations for future effect tagging
- Implementation: Lower vector operations to runtime intrinsics with explicit allocation sites
- Guarantee alignment: Future alloc effect enforcement can target intrinsic boundaries
- Validation: Vector creation, push, pop, index, iteration tests
- Validation: Capacity growth behavior tests for deterministic policy
- Exit criteria: Vector works with explicit allocation boundaries

## C1-WP16 Baseline collections: ordered map type
- Objective: Implement map<K,V> with stable iteration semantics by default
- Implementation: Use deterministic tree-based map in standard library core
- Implementation: Define ordering constraints on K and compile-time trait checks
- Implementation: Lower map operations to library calls with typed wrappers
- Guarantee alignment: Stable iteration order in deterministic contexts
- Validation: Insertion, lookup, remove, range iteration tests
- Validation: Same input yields same iteration order tests
- Exit criteria: Ordered map semantics complete and deterministic-ready

## C1-WP17 Baseline collections: unordered map type
- Objective: Implement unordered_map<K,V> with explicit nondeterministic caveat metadata
- Implementation: Use hash map runtime with seeded hash policy control hooks
- Implementation: Tag iteration as nondeterministic in metadata unless deterministic seed mode
- Implementation: Expose deterministic wrapper mode for future deterministic contexts
- Guarantee alignment: Deterministic checker can reject unsafe iteration usage later
- Validation: Hash collision behavior tests and API consistency tests
- Validation: Deterministic mode rejection tests prepared
- Exit criteria: Unordered map available with explicit semantic flags

## C1-WP18 References syntax and baseline typing
- Objective: Implement immutable and mutable reference syntax in parser and type system
- Implementation: Parse &T and ref T and represent mutability in type node
- Implementation: Add lvalue/rvalue classification in expression checker
- Implementation: Allow references in signatures and local bindings
- Guarantee alignment: Borrow checker can later attach lifetime and region constraints
- Validation: Reference formation and assignment legality tests
- Validation: Mixed mutable/immutable usage baseline tests
- Exit criteria: Reference types fully represented in typed AST and MIR

## C1-WP19 Raw pointers syntax and baseline typing
- Objective: Implement ptr<T> type parsing and basic operations syntax
- Implementation: Restrict dereference operations pending unsafe checker integration
- Implementation: Represent pointer arithmetic nodes with explicit unsafe-required marker
- Implementation: Lower raw pointer casts to explicit MIR opcodes
- Guarantee alignment: Raw pointer operations always traceable for unsafe enforcement
- Validation: Pointer type formation and cast tests
- Validation: Illegal safe-context dereference diagnostics tests
- Exit criteria: Pointer syntax integrated with future unsafe system

## C1-WP20 Pattern of deterministic evaluation order
- Objective: Lock expression evaluation order semantics in language core
- Implementation: Define left-to-right evaluation for operands and call arguments
- Implementation: Encode order in MIR sequence and prohibit backend reordering across side effects
- Implementation: Add effect barrier metadata for optimization passes
- Guarantee alignment: Determinism and reproducible side-effect sequencing
- Validation: Order-sensitive tests with side-effectful helper calls
- Validation: MIR instruction order invariance tests
- Exit criteria: Evaluation order immutable across optimization levels

## C1-WP21 MIR design baseline
- Objective: Create typed MIR with explicit control flow and operation effects slots
- Implementation: Use SSA-capable block structure with explicit terminators
- Implementation: Include per-instruction effect summary field for later checker passes
- Implementation: Include source span references for diagnostics and replay hashing
- Guarantee alignment: Effects and determinism checks can run on MIR uniformly
- Validation: MIR builder tests and block dominance tests
- Validation: Round-trip pretty-print tests for determinism
- Exit criteria: MIR stable and consumable by codegen

## C1-WP22 MIR lowering for control flow
- Objective: Lower high-level control flow to canonical branch and loop forms
- Implementation: Rewrite for loops into explicit iterator state blocks
- Implementation: Normalize break/continue into jump targets with phi merges
- Implementation: Preserve structured scope boundaries for later task/region checks
- Guarantee alignment: Later borrow and effect analyses operate on normalized CFG
- Validation: CFG normalization snapshot tests
- Validation: Scope boundary retention tests
- Exit criteria: Control flow lowering canonicalized

## C1-WP23 MIR lowering for data structures
- Objective: Lower struct and collection operations into explicit MIR operations
- Implementation: Expand struct initialization into field-wise writes
- Implementation: Represent map and vector calls as intrinsic call nodes
- Implementation: Mark potential allocation sites in MIR metadata
- Guarantee alignment: Allocation effect tracking starts in MIR
- Validation: MIR golden files for struct and collection heavy programs
- Validation: Allocation site tagging tests
- Exit criteria: Data structure lowering complete with metadata

## C1-WP24 LLVM type mapping baseline
- Objective: Map primitive and composite Sable types to LLVM types safely
- Implementation: Implement type lowering cache keyed by canonical type ID
- Implementation: Keep target ABI details centralized in backend ABI module
- Implementation: Defer complex layout transforms to Category 3 while keeping hooks
- Guarantee alignment: Predictable LLVM IR with stable type lowering
- Validation: LLVM type dump comparison tests
- Validation: ABI conformance tests for primitive and plain struct types
- Exit criteria: Baseline LLVM type lowering complete

## C1-WP25 LLVM codegen for functions and calls
- Objective: Emit LLVM functions, parameters, locals, and call instructions
- Implementation: Preserve source-level evaluation order in emitted instruction order
- Implementation: Attach debug locations and deterministic metadata tags
- Implementation: Generate function attributes based on known effects baseline
- Guarantee alignment: Side-effect visibility preserved and deterministic output support
- Validation: IR snapshot tests for call-heavy programs
- Validation: Rebuild bit-for-bit IR textual stability tests
- Exit criteria: Function codegen correct and reproducible

## C1-WP26 LLVM codegen for control flow and loops
- Objective: Emit branch and loop constructs with predictable structure
- Implementation: Map MIR blocks to LLVM basic blocks in stable order
- Implementation: Emit phi nodes deterministically using sorted predecessor IDs
- Implementation: Add canonical naming policy for temporary values
- Guarantee alignment: Stable IR generation and reproducible builds foundation
- Validation: Loop IR structure tests and phi correctness tests
- Validation: Block ordering stability tests
- Exit criteria: Control-flow codegen complete and deterministic-friendly

## C1-WP27 Runtime baseline: allocation API boundaries
- Objective: Implement runtime allocation entry points for vectors and maps
- Implementation: Expose alloc, realloc, and free with explicit region parameter placeholders
- Implementation: Ensure all frontend-generated allocations route through tracked runtime APIs
- Implementation: Add optional allocation event callback hook for verification mode
- Guarantee alignment: Absence/presence of alloc can be proven later
- Validation: Allocation accounting tests on compiled programs
- Validation: Leak-check harness integration tests
- Exit criteria: Allocation boundaries explicit and instrumentable

## C1-WP28 Runtime baseline: string and slice operations
- Objective: Implement deterministic string/slice primitives used by core language
- Implementation: Define UTF-8 validation and indexing semantics with explicit failure behavior
- Implementation: Keep deterministic behavior for operations independent of host locale
- Implementation: Provide pure and effectful variants where needed
- Guarantee alignment: Deterministic semantics and explicit error behavior groundwork
- Validation: Unicode edge-case and invalid sequence tests
- Validation: Locale independence tests
- Exit criteria: String core stable and deterministic-ready

## C1-WP29 Diagnostics framework
- Objective: Implement structured diagnostics with codes, spans, and notes
- Implementation: Create stable error code registry by category and checker
- Implementation: Render caret diagnostics with deterministic ordering
- Implementation: Support secondary labels and fix-it hints
- Guarantee alignment: Compile-time enforcement transparency and usability
- Validation: Golden diagnostic output tests
- Validation: Ordering determinism tests for multi-error files
- Exit criteria: Diagnostics production-quality for baseline passes

## C1-WP30 CLI and build pipeline baseline
- Objective: Build sable command for parse, check, build, and emit stages
- Implementation: Add subcommands for token dump, AST dump, MIR dump, and IR dump
- Implementation: Include deterministic mode flag placeholders with no-op warnings initially
- Implementation: Emit build manifest with pass versions and target details
- Guarantee alignment: Traceable compilation and future deterministic build controls
- Validation: CLI integration tests and option conflict tests
- Validation: Manifest schema tests
- Exit criteria: End-to-end compile pipeline usable by test suites

## C1-WP31 Baseline optimizer policy
- Objective: Introduce safe minimal optimizations without changing semantics
- Implementation: Implement constant folding for pure literal expressions only
- Implementation: Implement dead branch elimination for compile-time known conditions
- Implementation: Forbid optimizations that reorder potentially effectful operations
- Guarantee alignment: Zero hidden control flow changes violating semantics
- Validation: Semantic preservation tests and IR diff tests
- Validation: Effect barrier non-violation tests
- Exit criteria: Minimal optimization pipeline stable and sound

## C1-WP32 Determinism hooks in baseline backend
- Objective: Include determinism metadata plumbing before determinism checker is complete
- Implementation: Attach per-function deterministic eligibility placeholder in MIR
- Implementation: Attach per-call nondeterminism source tags where known
- Implementation: Export metadata section for future replay validator
- Guarantee alignment: Future determinism mode can be added without redesign
- Validation: Metadata emission snapshot tests
- Validation: Backward compatibility tests on old object readers
- Exit criteria: Determinism hooks present and tested

## C1-WP33 Baseline language conformance tests
- Objective: Build conformance suite for parser, sema basics, and codegen
- Implementation: Organize tests by grammar, typing, control flow, and collections
- Implementation: Include expected diagnostics and expected runtime outputs
- Implementation: Add deterministic expected output ordering checks
- Guarantee alignment: Prevent regressions against spec-defined core semantics
- Validation: CI run across debug and release compiler builds
- Validation: Differential run with optimization on and off
- Exit criteria: Conformance baseline green

## C1-WP34 Standard library baseline APIs
- Objective: Provide minimal standard library for vectors, maps, strings, and iterators
- Implementation: Separate pure APIs from effectful APIs with explicit annotations
- Implementation: Expose deterministic-friendly iterator variants by default
- Implementation: Document effect behavior in API docs for every function
- Guarantee alignment: Explicit effects and deterministic semantics accessibility
- Validation: API contract tests and documentation examples tests
- Validation: Effect annotation completeness checks
- Exit criteria: Baseline stdlib complete for core language use

## C1-WP35 Performance baseline and profiling harness
- Objective: Establish baseline compile-time and runtime performance tracking
- Implementation: Add deterministic benchmarking harness with fixed inputs and seeds
- Implementation: Track parser throughput, sema throughput, codegen throughput
- Implementation: Track runtime performance of vectors/maps with fixed datasets
- Guarantee alignment: Deterministic performance claims can be quantified
- Validation: Repeatability tests for benchmark medians
- Validation: Regression threshold checks in CI
- Exit criteria: Baseline performance dashboards in place

## C1-WP36 Category 1 release gate
- Objective: Define what must be complete before entering Category 2
- Implementation: Require all core grammar constructs compiled and tested
- Implementation: Require stable MIR and LLVM IR snapshots for core suite
- Implementation: Require deterministic ordering checks passing in baseline mode
- Guarantee alignment: Strong foundation for safety/effects systems
- Validation: Release checklist and sign-off from compiler and runtime leads
- Validation: Bug scrub and unresolved critical diagnostics count equals zero
- Exit criteria: Category 1 branch tagged and archived

# Category 2: Borrow Checking, Effects, Unsafe, Attributes, Errors, Comptime
## Scope
- Implement ownership and borrowing with regions
- Enforce function effects including raise effect
- Implement unsafe blocks and verified unsafe mode
- Implement basic attributes including deterministic and optimization controls baseline validation
- Implement explicit error handling with raise, catch, and abort semantics
- Implement compile-time execution and restrictions

## C2-WP01 Ownership model formalization in sema
- Objective: Encode single ownership and move semantics in typed MIR
- Implementation: Mark value-producing operations with ownership category
- Implementation: Insert move markers when values cross ownership boundaries
- Implementation: Reject implicit copies for non-copy types
- Guarantee alignment: Memory safety and no hidden ownership transfer
- Validation: Move-after-use and double-move negative tests
- Validation: Explicit clone legality tests
- Exit criteria: Ownership tracking present on all MIR values

## C2-WP02 Borrow checker constraint graph
- Objective: Build borrow checker over CFG with non-lexical lifetime inference
- Implementation: Create borrow facts for borrow start, use, and end points
- Implementation: Solve constraints using dataflow fixed-point iteration
- Implementation: Track mutable and immutable borrow compatibility per location
- Guarantee alignment: One mutable or many immutable rule
- Validation: Classic aliasing negative tests and accepted patterns tests
- Validation: Non-lexical lifetime acceptance tests
- Exit criteria: Borrow constraints solved for all functions

## C2-WP03 Region-annotated reference model
- Objective: Extend references with region metadata and checks
- Implementation: Parse and type-check ref<region> forms
- Implementation: Attach region IDs to reference types in interner
- Implementation: Enforce region compatibility on assignments and returns
- Guarantee alignment: No cross-region leaks and valid lifetimes
- Validation: Region mismatch diagnostics tests
- Validation: Region polymorphism baseline tests if supported
- Exit criteria: Region-aware references semantically enforced

## C2-WP04 Region allocation semantics
- Objective: Implement region declarations and region-scoped allocation semantics
- Implementation: Lower region declarations to runtime region handles
- Implementation: Enforce allocations carry region ownership metadata
- Implementation: Enforce O(1) region free by block-exit drop insertion
- Guarantee alignment: Region lifetime guarantees and non-escaping values
- Validation: Escape analysis tests for region-allocated values
- Validation: Region teardown complexity benchmarks
- Exit criteria: Region allocation and teardown semantics complete

## C2-WP05 Region escape analysis
- Objective: Ensure values allocated in a region cannot escape its lifetime
- Implementation: Run escape analysis on return values, captured closures, and globals
- Implementation: Reject storing region-bound refs into longer-lived storage
- Implementation: Integrate with borrow checker to block indirect escapes
- Guarantee alignment: Values cannot escape region
- Validation: Positive and negative escape examples
- Validation: Indirect alias escape tests
- Exit criteria: Escape violations diagnosed reliably

## C2-WP06 Effect system core representation
- Objective: Represent effects as first-class sets in type and MIR signatures
- Implementation: Define effect lattice with none as empty set
- Implementation: Include built-in effects alloc, io, mut, unsafe
- Implementation: Include parametric raise effects with error type list
- Guarantee alignment: Effects part of type system and transitive enforcement
- Validation: Effect set equality and subset tests
- Validation: Function type identity tests including effects
- Exit criteria: Effect representation complete in sema and MIR

## C2-WP07 Effect inference and propagation pass
- Objective: Infer and propagate effects through function bodies and call graph
- Implementation: Collect intrinsic and runtime call effect summaries
- Implementation: Propagate callee effects into caller required effects
- Implementation: Handle recursion via SCC fixed-point solving
- Guarantee alignment: Effects are transitive and violations are compile errors
- Validation: Recursive function effect inference tests
- Validation: Missing effect annotation diagnostics tests
- Exit criteria: Correct effect closure computed for all functions

## C2-WP08 effects(none) strict verifier
- Objective: Enforce no alloc, no io, no external mutation, and deterministic eligibility
- Implementation: Scan MIR for any effectful opcodes or effectful calls
- Implementation: Verify no allocation intrinsics reachable
- Implementation: Verify no io intrinsics or foreign side effects reachable
- Guarantee alignment: effects(none) guarantees side-effect free and allocation free
- Validation: Negative tests for hidden allocation and io in helper calls
- Validation: Positive pure function test suite
- Exit criteria: effects(none) verifier blocks all violations

## C2-WP09 Effect-aware function type checking
- Objective: Ensure function values include exact effect set compatibility rules
- Implementation: Enforce assignment compatibility by effect subset/superset policy
- Implementation: Reject passing effectful function where pure function type expected
- Implementation: Include raise effects in compatibility checks
- Guarantee alignment: Effect sets are part of function types
- Validation: Higher-order function effect mismatch tests
- Validation: Callback and closure effect tests
- Exit criteria: Function type checker effect-aware and complete

## C2-WP10 Unsafe block parser and semantic boundaries
- Objective: Implement unsafe blocks and unsafe context tracking
- Implementation: Parse unsafe block AST nodes and attach spans
- Implementation: Mark MIR instructions originating in unsafe contexts
- Implementation: Reject unsafe operations outside unsafe contexts
- Guarantee alignment: Unsafe operations explicitly marked
- Validation: Raw pointer dereference legality tests
- Validation: Unsafe boundary diagnostics tests
- Exit criteria: Unsafe boundary enforcement complete

## C2-WP11 Verified unsafe mode instrumentation
- Objective: Implement unsafe @verify instrumentation path
- Implementation: Inject bounds checks for pointer and index operations in unsafe blocks
- Implementation: Inject lifetime validity checks for region and borrow references
- Implementation: Inject optional race detector hooks for unsafe concurrent accesses
- Guarantee alignment: Seatbelt mode checks only unsafe regions
- Validation: Verified unsafe runtime fault tests
- Validation: Performance overhead benchmarks with and without verify
- Exit criteria: Verified unsafe mode reliable and optional

## C2-WP12 Basic attribute framework
- Objective: Implement generic attribute parsing, validation, and storage
- Implementation: Define attribute registry with target kinds and argument schemas
- Implementation: Validate duplicates and conflicting attributes
- Implementation: Normalize attribute args to canonical order
- Guarantee alignment: Attribute semantics enforceable and deterministic
- Validation: Attribute parser and schema mismatch tests
- Validation: Duplicate/conflict diagnostics tests
- Exit criteria: Attribute engine supports current and future attributes

## C2-WP13 @deterministic basic checker hooks
- Objective: Implement base deterministic checker for prohibited constructs
- Implementation: Reject direct io and disallowed unsafe in deterministic functions
- Implementation: Tag nondeterministic collection iteration sources
- Implementation: Reject wall-clock and randomness intrinsics by default
- Guarantee alignment: Deterministic restrictions enforced
- Validation: Deterministic rejection tests for forbidden operations
- Validation: Deterministic acceptance tests for pure computations
- Exit criteria: Basic deterministic checker active

## C2-WP14 @opt attribute baseline behavior
- Objective: Implement per-function optimization contract parsing and backend plumbing
- Implementation: Parse @opt(level=N, deterministic=bool) arguments
- Implementation: Apply pass pipeline selection at function granularity
- Implementation: Enforce deterministic option stable pass ordering
- Guarantee alignment: Optimization contracts override CLI safely
- Validation: Function-level optimization behavior tests
- Validation: Deterministic build stability tests with @opt deterministic=true
- Exit criteria: @opt baseline functional and validated

## C2-WP15 Error type declarations and hierarchy
- Objective: Implement user-defined error types for raise effects
- Implementation: Add error type kind and variant support in type system
- Implementation: Require errors to be nominal and exhaustively matchable
- Implementation: Generate compact runtime representations for error values
- Guarantee alignment: Explicit and traceable error typing
- Validation: Error declaration and construction tests
- Validation: Invalid error usage diagnostics tests
- Exit criteria: Error types integrated into sema and MIR

## C2-WP16 raise statement semantics
- Objective: Implement raise operation and control transfer modeling
- Implementation: Lower raise to MIR exceptional edge with typed payload
- Implementation: Ensure raise exits current function path immediately
- Implementation: Record raised error type in function effect summary
- Guarantee alignment: Explicit failure paths and effect propagation
- Validation: Raise control flow tests and dead-code tests after raise
- Validation: Missing raise effect diagnostics tests
- Exit criteria: raise semantics complete

## C2-WP17 try/catch semantics and effect discharge
- Objective: Implement try/catch with typed catch handlers
- Implementation: Lower try/catch to MIR regions with exceptional control edges
- Implementation: Remove caught error types from outgoing effect set
- Implementation: Enforce catch typing and handler exhaustiveness where applicable
- Guarantee alignment: Catching removes errors from effect set
- Validation: Partial handling and full handling tests
- Validation: Effect discharge correctness tests
- Exit criteria: try/catch behavior spec-compliant

## C2-WP18 Unhandled error propagation diagnostics
- Objective: Detect unhandled recoverable errors at compile time
- Implementation: Compare function body raised set minus caught set against signature
- Implementation: Emit diagnostics listing exact unhandled error variants
- Implementation: Provide fix hints to add catch or propagate effects
- Guarantee alignment: No unhandled recoverable errors in well-typed programs
- Validation: Negative tests for missing handlers
- Validation: Diagnostics quality tests with multi-error paths
- Exit criteria: Unhandled error pass complete

## C2-WP19 abort semantics and runtime behavior
- Objective: Implement abort for fatal unrecoverable failures
- Implementation: Lower abort to runtime trap task-termination primitive
- Implementation: In managed contexts expose rollback or controlled shutdown hook
- Implementation: Exclude abort from required effect declarations
- Guarantee alignment: Fatal errors immediate and explicit
- Validation: Abort behavior tests in normal and sandbox contexts
- Validation: Ensure abort not required in effect signatures
- Exit criteria: abort semantics complete and documented

## C2-WP20 Effect constraints inside catch blocks
- Objective: Prevent catch blocks from introducing undeclared effects
- Implementation: Type-check catch body under enclosing effect budget
- Implementation: Track nested calls and allocations inside handlers
- Implementation: Emit compile errors when handler introduces forbidden effects
- Guarantee alignment: Handlers cannot exceed enclosing scope effects
- Validation: Allowed and forbidden catch side-effect tests
- Validation: Diagnostic clarity tests
- Exit criteria: Catch effect budget enforcement complete

## C2-WP21 Compile-time execution engine architecture
- Objective: Implement @comp evaluator with deterministic interpreter
- Implementation: Build MIR interpreter for subset allowed at compile time
- Implementation: Separate compile-time heap from runtime heap
- Implementation: Cache compile-time evaluation results by normalized inputs
- Guarantee alignment: Compile-time execution deterministic and constrained
- Validation: @comp expression and declaration tests
- Validation: Cache determinism tests
- Exit criteria: Compile-time engine operational

## C2-WP22 @comp restriction enforcement
- Objective: Enforce no io and restricted alloc rules in compile-time code
- Implementation: Reuse effect checker in compile-time mode with stricter policy
- Implementation: Permit alloc only with explicit compiler flag/attribute gate
- Implementation: Reject non-deterministic intrinsics in compile-time context
- Guarantee alignment: @comp rules from spec enforced
- Validation: Forbidden io and alloc negative tests
- Validation: Deterministic compile-time replay tests
- Exit criteria: @comp restriction checker complete

## C2-WP23 Compile-time value serialization
- Objective: Materialize compile-time values into constants in emitted code
- Implementation: Serialize primitives, structs, arrays, and maps where legal
- Implementation: Canonicalize serialization ordering for deterministic builds
- Implementation: Validate no runtime pointers leak from compile-time heap
- Guarantee alignment: Deterministic and safe compile-time materialization
- Validation: Constant emission and binary reproducibility tests
- Validation: Illegal value materialization diagnostics tests
- Exit criteria: Compile-time value lowering stable

## C2-WP24 Borrow checker diagnostics quality pass
- Objective: Improve usability of ownership and borrow errors
- Implementation: Emit origin and conflicting use spans with explanatory notes
- Implementation: Suggest move, clone, reborrow, or scope narrowing fixes
- Implementation: Group repeated diagnostics to reduce noise
- Guarantee alignment: Practical enforceability of safety by construction
- Validation: Golden diagnostics tests on representative borrow failures
- Validation: Developer feedback loop from sample projects
- Exit criteria: Borrow diagnostics production quality

## C2-WP25 Region and borrow MIR annotations
- Objective: Preserve borrow and region facts in MIR for backend and verification
- Implementation: Attach lifetime intervals to reference values
- Implementation: Attach region ownership transitions to move operations
- Implementation: Attach unsafe-origin flags to pointer operations
- Guarantee alignment: End-to-end safety reasoning across pipeline
- Validation: MIR annotation integrity tests
- Validation: Annotation preservation tests through optimization
- Exit criteria: Borrow/region metadata stable across passes

## C2-WP26 Basic attribute set completion
- Objective: Fully implement basic attributes required before advanced phase
- Implementation: Finalize @deterministic, @opt, @comp parse and sema behavior
- Implementation: Add placeholder parse support for @hot, @abi, @verify_layout, @layout
- Implementation: Reserve conflict checks for advanced attributes
- Guarantee alignment: Smooth path to Category 3 without syntax changes
- Validation: Attribute compatibility matrix tests
- Validation: Placeholder diagnostics tests for not-yet-enabled semantics
- Exit criteria: Basic attribute system complete

## C2-WP27 Effect summaries for stdlib
- Objective: Annotate and verify standard library functions with explicit effects
- Implementation: Mark every stdlib API with effect declarations including raise where needed
- Implementation: Run effect inference to verify declared summaries
- Implementation: Block undocumented effects in stdlib CI
- Guarantee alignment: No hidden io/alloc/mut in standard APIs
- Validation: Effect summary conformance tests
- Validation: Regression tests for accidental effect drift
- Exit criteria: Stdlib effects audited and enforced

## C2-WP28 Effect-aware optimization constraints
- Objective: Prevent optimizer from violating effect and error semantics
- Implementation: Treat effectful operations as scheduling barriers
- Implementation: Preserve raise control-flow edges during transforms
- Implementation: Maintain catch region boundaries and handler mapping
- Guarantee alignment: No hidden control flow or side-effect reordering
- Validation: Optimization semantic preservation tests
- Validation: Differential execution tests with random programs
- Exit criteria: Optimizer effect-safe

## C2-WP29 Deterministic preliminary floating-point policy
- Objective: Establish deterministic FP baseline before full deterministic mode
- Implementation: Define default FP flags for predictable semantics in deterministic-tagged code
- Implementation: Disable backend fast-math in deterministic contexts
- Implementation: Document architecture constraints for bitwise reproducibility
- Guarantee alignment: Controlled floating-point semantics groundwork
- Validation: Cross-machine deterministic FP tests on supported targets
- Validation: Fast-math rejection tests in deterministic contexts
- Exit criteria: Deterministic FP baseline enforced

## C2-WP30 Semantic query tooling
- Objective: Provide compiler queries for effects and error propagation
- Implementation: Add CLI commands to list functions by effect and raise set
- Implementation: Add query for unhandled error paths
- Implementation: Add query for io operations lacking error propagation
- Guarantee alignment: Tooling guarantees from spec section 19.12
- Validation: Query correctness tests and snapshot outputs
- Validation: Large project performance tests for query mode
- Exit criteria: Semantic tooling ready for developer workflows

## C2-WP31 Verification mode runtime library
- Objective: Build runtime helpers for verify checks in unsafe and sandbox contexts
- Implementation: Add bounds check stubs, lifetime map, and pointer provenance tracker
- Implementation: Add optional race detector integration points
- Implementation: Add configuration to scope checks only to instrumented regions
- Guarantee alignment: Verification applies only where requested
- Validation: Verify scope tests and overhead measurements
- Validation: Correct fault attribution tests
- Exit criteria: Verification runtime complete

## C2-WP32 Category 2 conformance suite
- Objective: Build full test suite for ownership, effects, errors, and @comp
- Implementation: Add compile-fail suites for borrow/effect/error violations
- Implementation: Add run-pass suites for legal borrow and handler patterns
- Implementation: Add deterministic stability checks for @comp results
- Guarantee alignment: Safety and explicit effects credibly enforced
- Validation: CI matrix across targets and optimization levels
- Validation: Nightly stress and fuzz runs
- Exit criteria: Category 2 test gate green

## C2-WP33 Category 2 performance and scalability
- Objective: Ensure sema checks scale to large codebases
- Implementation: Profile borrow and effect passes; optimize hot paths
- Implementation: Introduce incremental analysis caches keyed by stable IDs
- Implementation: Keep cache invalidation deterministic and precise
- Guarantee alignment: Deterministic performance and practical compile times
- Validation: Large synthetic project benchmarks
- Validation: Incremental compile determinism tests
- Exit criteria: Performance targets met

## C2-WP34 Category 2 release gate
- Objective: Freeze semantics for ownership/effects/error/core attributes
- Implementation: Sign off formal checker invariants and diagnostics behavior
- Implementation: Publish migration guide from Category 1 baseline
- Implementation: Lock MIR metadata schema for advanced features
- Guarantee alignment: Stable foundation for advanced systems
- Validation: End-to-end language test battery and bug scrub
- Validation: Release candidate reproducibility audit
- Exit criteria: Category 2 branch tagged and archived

# Category 3: FFI, Sandboxing, Recording, Determinism, Hot Reload, Complex Attributes, Concurrency, SIMD, Cache
## Scope
- Implement advanced attributes and runtime systems
- Complete deterministic execution model and replay guarantees
- Implement FFI with effect inference and verification
- Implement structured concurrency and deterministic parallel semantics
- Implement hot reload and semantic binary patching
- Implement data layout transformations, bit structs, SIMD, and cache controls

## C3-WP01 Full determinism checker architecture
- Objective: Build deterministic checker as dedicated pass over MIR and call graph
- Implementation: Classify operations by deterministic safety class
- Implementation: Reject nondeterministic primitives and unstable iteration sources
- Implementation: Track deterministic obligations transitively across calls
- Guarantee alignment: Deterministic contexts reject nondeterministic constructs
- Validation: Deterministic positive/negative suite with mixed modules
- Validation: Call graph transitivity tests
- Exit criteria: Determinism checker complete and integrated

## C3-WP02 Deterministic module-level enforcement
- Objective: Support @deterministic at module granularity
- Implementation: Propagate deterministic requirement to all contained functions
- Implementation: Enforce import boundaries and deterministic dependency checks
- Implementation: Reject calls into non-deterministic modules unless whitelisted adapters
- Guarantee alignment: Module deterministic guarantees maintain closure
- Validation: Module annotation propagation tests
- Validation: Boundary violation diagnostics tests
- Exit criteria: Module-level deterministic mode operational

## C3-WP03 Deterministic scheduling contract
- Objective: Define runtime scheduling semantics for deterministic contexts
- Implementation: Keep scheduling implementation-defined but result-constrained
- Implementation: Introduce deterministic join/merge ordering by lexical spawn index
- Implementation: Record and enforce stable reduction order for parallel results
- Guarantee alignment: Identical observable behavior across runs
- Validation: Parallel deterministic output equivalence tests
- Validation: Stress tests with varied worker counts
- Exit criteria: Deterministic scheduling contract implemented

## C3-WP04 Wall-clock and randomness gating
- Objective: Block nondeterministic environmental sources in deterministic code
- Implementation: Gate time APIs and random APIs behind non-deterministic effect domains
- Implementation: Provide deterministic injection API for seeded or provided values
- Implementation: Tag forbidden intrinsics and reject in deterministic checker
- Guarantee alignment: No wall-clock or random nondeterminism in deterministic contexts
- Validation: Forbidden API call diagnostics tests
- Validation: Deterministic injection replay tests
- Exit criteria: Environmental nondeterminism controls complete

## C3-WP05 Deterministic floating-point complete policy
- Objective: Finalize cross-platform deterministic FP behavior
- Implementation: Define supported target matrix for bitwise FP reproducibility
- Implementation: Enforce strict IEEE modes and disable target-specific contractions
- Implementation: Add compiler flag to fail build on unsupported deterministic FP target
- Guarantee alignment: Bitwise reproducible deterministic execution
- Validation: Cross-machine FP reproducibility suite
- Validation: Unsupported target rejection tests
- Exit criteria: Deterministic FP policy final and enforced

## C3-WP06 FFI declaration parser and type bridge
- Objective: Implement extern declarations and ABI-bound type checks
- Implementation: Parse extern language specifier and function signatures
- Implementation: Validate supported ABI strings and type compatibility
- Implementation: Generate bridge stubs for marshalling when required
- Guarantee alignment: Safe and explicit foreign function integration
- Validation: Extern parsing tests and ABI mismatch diagnostics
- Validation: Bridge call correctness tests
- Exit criteria: Core FFI declaration support complete

## C3-WP07 FFI static effect inference engine
- Objective: Infer effects of foreign symbols when analyzable
- Implementation: Analyze IR/bitcode or metadata for alloc, io, mut, unsafe behaviors
- Implementation: Build conservative summary model including pointer writes
- Implementation: Cache summaries keyed by symbol hash and binary version
- Guarantee alignment: Foreign effects integrate with Sable effect system
- Validation: Known C library wrapper inference tests
- Validation: Cache correctness and invalidation tests
- Exit criteria: FFI effect inference available and stable

## C3-WP08 FFI pessimistic fallback behavior
- Objective: Apply maximal effect set when analysis incomplete
- Implementation: Assign alloc, io, mut, unsafe when inference confidence insufficient
- Implementation: Emit informational diagnostic recommending explicit annotation or verify
- Implementation: Preserve fallback summary in API metadata
- Guarantee alignment: No unsound under-approximation of foreign effects
- Validation: Inference-failure fallback tests
- Validation: Caller effect requirement propagation tests
- Exit criteria: Pessimistic fallback guaranteed sound

## C3-WP09 FFI explicit effect annotation verification
- Objective: Validate user-declared FFI effects against analysis or runtime checks
- Implementation: Compare declared effects to inferred effects when available
- Implementation: Emit mismatch diagnostics with specific violating behavior class
- Implementation: Allow @verify runtime checks where static proof unavailable
- Guarantee alignment: Declared foreign effects trustworthy when verified
- Validation: Annotation match/mismatch test suite
- Validation: Runtime verify path tests
- Exit criteria: FFI annotation verification complete

## C3-WP10 Sandbox runtime core
- Objective: Implement sandbox execution wrapper for restricted code
- Implementation: Provide syscall filtering and policy tables by effect budget
- Implementation: Track memory writes to global regions and reject forbidden writes
- Implementation: Restrict allocations to allowed regions in verify mode
- Guarantee alignment: Sandbox guarantees for io, mutation, allocation boundaries
- Validation: Sandbox escape attempt tests
- Validation: Policy enforcement latency and overhead tests
- Exit criteria: Sandbox runtime core stable

## C3-WP11 Sandbox compiler integration
- Objective: Integrate sandbox blocks with sema and codegen
- Implementation: Parse sandbox blocks and lower to guarded runtime invocation
- Implementation: Infer and check effect budget inside sandbox scope
- Implementation: Ensure abort behavior triggers controlled rollback semantics
- Guarantee alignment: Sandbox behavior predictable and safe
- Validation: Nested sandbox and error interaction tests
- Validation: Rollback correctness tests
- Exit criteria: Sandbox language integration complete

## C3-WP12 Structured concurrency runtime
- Objective: Implement lexical-scope-bound task system
- Implementation: Add spawn API that registers child tasks in scope task group
- Implementation: Ensure all spawned tasks join before scope exit
- Implementation: Ensure panic/abort handling propagates with structured semantics
- Guarantee alignment: Structured concurrency and no detached hidden tasks
- Validation: Scope-exit join enforcement tests
- Validation: Task failure propagation tests
- Exit criteria: Structured task runtime complete

## C3-WP13 Concurrency capture and ownership checks
- Objective: Enforce capture constraints for spawned tasks
- Implementation: Allow immutable captures, moved values, and moved regions only
- Implementation: Reject shared mutable access without synchronization primitive policy
- Implementation: Integrate borrow checker with task boundary lifetime checks
- Guarantee alignment: Data race freedom in safe code
- Validation: Illegal capture compile-fail tests
- Validation: Legal move and immutable capture tests
- Exit criteria: Spawn capture checks complete

## C3-WP14 Region transfer in spawn
- Objective: Implement spawn(task, move region) semantics
- Implementation: Transfer region ownership token to child task context
- Implementation: Invalidate sender references and borrows to transferred region
- Implementation: Reject transfer if outstanding borrows exist
- Guarantee alignment: Exclusive ownership transfer and no dangling refs
- Validation: Transfer success and failure tests
- Validation: Post-transfer use diagnostics tests
- Exit criteria: Region transfer semantics complete

## C3-WP15 Deterministic concurrency restrictions
- Objective: Enforce deterministic context bans on locks and nondeterministic primitives
- Implementation: Reject mutex/lock APIs in deterministic scopes
- Implementation: Reject unsynchronized shared mutation at sema level
- Implementation: Permit only deterministic-safe parallel patterns
- Guarantee alignment: Deterministic concurrency restrictions from spec
- Validation: Forbidden primitive diagnostics tests
- Validation: Allowed deterministic parallel reductions tests
- Exit criteria: Deterministic concurrency checker complete

## C3-WP16 Data layout attribute core
- Objective: Implement @layout(AoS) and @layout(SoA) attribute semantics
- Implementation: Parse and validate layout attributes on structs
- Implementation: Build layout transformation descriptors in type system
- Implementation: Lower accesses according to transformed layout without runtime overhead
- Guarantee alignment: Data-oriented defaults and zero-cost abstraction
- Validation: Layout transform correctness tests
- Validation: Runtime equivalence tests AoS vs SoA semantics
- Exit criteria: Layout attributes fully operational

## C3-WP17 SoA transformation lowering
- Objective: Transform struct storage into parallel arrays at compile time
- Implementation: Generate synthetic storage types for each field array
- Implementation: Rewrite field access into array-indexed operations
- Implementation: Preserve aliasing and borrow semantics through rewritten paths
- Guarantee alignment: SoA compile-time rewrite with no runtime overhead
- Validation: SoA rewrite MIR and LLVM snapshot tests
- Validation: Borrow checker interaction tests with rewritten accesses
- Exit criteria: SoA lowering robust and verified

## C3-WP18 Layout reflection API
- Objective: Implement layout_of reflection in compile-time context
- Implementation: Expose size, align, and field offset metadata objects
- Implementation: Restrict reflection to compile-time-safe deterministic evaluation
- Implementation: Ensure reflected values match backend ABI and layout transforms
- Guarantee alignment: Layout reflection guarantees correctness and predictability
- Validation: layout_of tests for plain and transformed structs
- Validation: Cross-target layout consistency tests where required
- Exit criteria: Reflection API complete

## C3-WP19 @transform attribute implementation
- Objective: Implement composable transform attribute for layout and alignment
- Implementation: Parse transform arguments and validate combinations
- Implementation: Apply transform pipeline in canonical order
- Implementation: Emit diagnostics for incompatible transform stacks
- Guarantee alignment: Programmable memory layout with compile-time checks
- Validation: Transform composition tests
- Validation: Invalid combination diagnostics tests
- Exit criteria: @transform feature complete

## C3-WP20 @bits struct implementation
- Objective: Implement bit-level struct declarations with exact packing
- Implementation: Parse fixed-width integer fields and compute packed offsets
- Implementation: Generate accessor and mutator code with masking/shifting
- Implementation: Reject unsupported widths and overflow field totals
- Guarantee alignment: Exact bit layout, no padding, safe field access
- Validation: Packed representation tests and round-trip field tests
- Validation: Endianness handling tests per target policy
- Exit criteria: @bits fully implemented

## C3-WP21 ABI contracts and @abi(C)
- Objective: Implement ABI annotation and external layout constraints
- Implementation: Tag types/functions with ABI domain metadata
- Implementation: Use ABI metadata in codegen function signatures and struct layout
- Implementation: Validate unsupported ABI usage with precise diagnostics
- Guarantee alignment: Exact binary interop contracts
- Validation: C interop tests with compiled C harness
- Validation: ABI mismatch rejection tests
- Exit criteria: ABI contract support complete

## C3-WP22 @verify_layout implementation
- Objective: Validate type size and alignment at compile time
- Implementation: Parse size and align constraints and compare to computed layout
- Implementation: Run checks after all transforms and ABI rules applied
- Implementation: Emit detailed mismatch diagnostics including expected/actual values
- Guarantee alignment: Compile-time layout validation guarantee
- Validation: Passing and failing verify_layout tests
- Validation: Cross-target validation tests for supported targets
- Exit criteria: verify_layout complete

## C3-WP23 Semantic binary patch metadata emission
- Objective: Emit structural metadata necessary for patch generation
- Implementation: Store type layout hashes, function signature hashes, and symbol maps
- Implementation: Canonicalize metadata ordering for deterministic patch files
- Implementation: Version metadata schema and compatibility rules
- Guarantee alignment: Verified semantic patch generation foundation
- Validation: Metadata diff stability tests
- Validation: Backward compatibility tests across compiler versions
- Exit criteria: Patch metadata emission complete

## C3-WP24 Patch generation command implementation
- Objective: Implement sable build --patch-from behavior
- Implementation: Compare prior and current metadata for compatibility
- Implementation: Generate minimal binary diff and semantic manifest
- Implementation: Fail generation on incompatible layout or signature changes
- Guarantee alignment: Safe patch generation with rejection conditions
- Validation: Compatible and incompatible patch generation tests
- Validation: Patch size and correctness benchmarks
- Exit criteria: Patch generation pipeline complete

## C3-WP25 Patch application runtime
- Objective: Apply patches atomically with integrity validation and rollback
- Implementation: Verify patch signature and metadata compatibility before apply
- Implementation: Swap code/data sections using transactional protocol
- Implementation: Roll back atomically on verification or apply failure
- Guarantee alignment: Safe atomic patching and rollback support
- Validation: Power-failure simulation and partial apply tests
- Validation: Rollback correctness tests
- Exit criteria: Patch apply runtime production-ready

## C3-WP26 Hot reload eligibility checker
- Objective: Determine if @hot functions satisfy reload safety criteria
- Implementation: Enforce signature and ABI identity across versions
- Implementation: Enforce no unsafe and no forbidden effects in hot functions
- Implementation: Enforce compatible dependent type layouts
- Guarantee alignment: Hot reload rejection on unsafe incompatibilities
- Validation: Eligibility pass/fail tests
- Validation: Layout dependency change rejection tests
- Exit criteria: Hot reload eligibility checker complete

## C3-WP27 Hot reload function table runtime
- Objective: Implement versioned function pointer table and atomic swap
- Implementation: Route hot-callable functions through indirection table entries
- Implementation: Swap entry pointers atomically at safe boundary transitions
- Implementation: Keep previous version for rollback on failure
- Guarantee alignment: Future calls use new implementation without corrupting active frames
- Validation: Concurrent call and reload stress tests
- Validation: Version rollback tests
- Exit criteria: Hot reload runtime core complete

## C3-WP28 Hot reload boundary semantics
- Objective: Enforce replacement only at function boundary transitions
- Implementation: Track active frames and disallow mid-frame mutation
- Implementation: Defer activation until safe transition point
- Implementation: Document exact semantics for in-flight calls
- Guarantee alignment: No mid-instruction replacement and stack frame validity
- Validation: Boundary transition tests with deep recursion
- Validation: In-flight behavior tests
- Exit criteria: Boundary semantics implemented and verified

## C3-WP29 Hot reload error compatibility
- Objective: Validate raise-effect compatibility between hot versions
- Implementation: Compare old/new raise sets and block incompatible additions
- Implementation: Offer optional runtime rollback if strict mode allows tentative load
- Implementation: Emit detailed compatibility diagnostics
- Guarantee alignment: No hidden new error behavior across reload
- Validation: Raise-set compatibility tests
- Validation: Rollback-on-incompatibility tests
- Exit criteria: Error compatibility checks complete

## C3-WP30 Recording API implementation
- Objective: Implement recorder.capture and recording artifact format
- Implementation: Capture initial inputs, code version metadata, and optional checksum
- Implementation: Avoid tracing full runtime when deterministic guarantees hold
- Implementation: Serialize recording with canonical format and stable field order
- Guarantee alignment: Minimal deterministic recording model
- Validation: Recording serialization and deserialization tests
- Validation: Version metadata completeness tests
- Exit criteria: Capture API complete

## C3-WP31 Replay engine implementation
- Objective: Implement recorder.replay for deterministic modules
- Implementation: Load recording, validate compatibility, execute target entrypoint
- Implementation: Verify optional checksum and output equality
- Implementation: Emit mismatch diagnostics with deterministic violation hints
- Guarantee alignment: Exact replay for deterministic code
- Validation: Replay success tests across repeated runs
- Validation: Replay rejection tests for mismatched versions
- Exit criteria: Replay engine complete

## C3-WP32 Replay environment compatibility checks
- Objective: Validate architecture and environment constraints during replay
- Implementation: Compare target architecture normalization tags
- Implementation: Compare compiler/runtime deterministic policy versions
- Implementation: Reject replay when constraints differ beyond allowed tolerance
- Guarantee alignment: Failure modes match spec
- Validation: Environment mismatch rejection tests
- Validation: Allowed compatibility mode tests
- Exit criteria: Replay compatibility checker complete

## C3-WP33 Deterministic checksum model
- Objective: Define deterministic execution checksum for optional validation
- Implementation: Hash canonicalized state transitions or outputs at checkpoints
- Implementation: Exclude nondeterministic metadata fields from checksum input
- Implementation: Version checksum algorithm with migration support
- Guarantee alignment: Replay validation and reproducibility confidence
- Validation: Checksum stability tests across builds
- Validation: Mismatch detection tests
- Exit criteria: Checksum model production-ready

## C3-WP34 Complex attribute conflict system
- Objective: Enforce interactions among layout, abi, opt, deterministic, hot attributes
- Implementation: Build declarative conflict matrix and checker pass
- Implementation: Support target-specific conditional constraints
- Implementation: Emit actionable conflict diagnostics
- Guarantee alignment: Attribute semantics remain coherent and safe
- Validation: Conflict matrix tests
- Validation: Multi-attribute integration tests
- Exit criteria: Attribute conflict checker complete

## C3-WP35 SIMD attribute lowering
- Objective: Implement @simd(width=N) function lowering strategy
- Implementation: Validate width against target vector capabilities
- Implementation: Generate vectorized loop/body variants with fallback when unsupported
- Implementation: Ensure deterministic mode disallows nondeterministic vector reductions
- Guarantee alignment: SIMD control explicit and predictable
- Validation: Vectorized correctness and speed tests
- Validation: Deterministic-mode SIMD constraints tests
- Exit criteria: SIMD lowering complete

## C3-WP36 Cache alignment attribute lowering
- Objective: Implement @cache(line=N) layout and allocation behavior
- Implementation: Validate cache line values and alignment constraints
- Implementation: Adjust type alignment and allocator behavior accordingly
- Implementation: Preserve ABI checks and verify_layout interactions
- Guarantee alignment: Explicit cache control with layout guarantees
- Validation: Alignment and padding tests
- Validation: ABI compatibility tests with cache attribute
- Exit criteria: Cache control attribute complete

## C3-WP37 Tiling construct implementation
- Objective: Implement data.tile(N) iteration lowering and runtime support
- Implementation: Type-check tiling APIs and tile-size constraints
- Implementation: Lower tile loops into nested loop kernels with bounds handling
- Implementation: Provide deterministic iteration order guarantees
- Guarantee alignment: Data-oriented and deterministic loop transformations
- Validation: Tile boundary correctness tests
- Validation: Performance tests for cache locality gains
- Exit criteria: Tiling semantics complete

## C3-WP38 Advanced optimizer with contracts
- Objective: Honor optimization contracts while preserving guarantees
- Implementation: Respect @opt level and deterministic flag per function/module
- Implementation: Disable or constrain passes conflicting with deterministic guarantees
- Implementation: Keep pass pipeline reproducible and version-locked
- Guarantee alignment: Optimization without semantic drift
- Validation: Contract adherence tests and IR pipeline snapshot tests
- Validation: Deterministic build bitwise tests under varied opt levels
- Exit criteria: Contract-aware optimizer complete

## C3-WP39 Deterministic build reproducibility system
- Objective: Deliver reproducible builds under deterministic mode
- Implementation: Normalize timestamps, UUIDs, and debug path mappings
- Implementation: Canonicalize section ordering and symbol ordering
- Implementation: Emit reproducibility manifest containing toolchain identifiers
- Guarantee alignment: Stable IR generation and reproducible builds guarantees
- Validation: Bit-for-bit build reproducibility tests on same toolchain version
- Validation: Manifest verification tests
- Exit criteria: Reproducible build mode complete

## C3-WP40 Final conformance and certification gate
- Objective: Certify language implementation against full spec guarantees
- Implementation: Run complete conformance, fuzzing, stress, and determinism suites
- Implementation: Audit unresolved bugs against guarantee-impact rubric
- Implementation: Produce certification report mapping each guarantee to test evidence
- Guarantee alignment: Full spec coverage with evidence
- Validation: Independent rerun by release engineering
- Validation: Public test artifacts and reproducibility logs
- Exit criteria: Sable implementation declared spec-complete

# Milestone Breakdown
## Milestone M0: Boot and architecture
- Deliver source manager, lexer skeleton, parser skeleton
- Deliver deterministic ordering and ID primitives
- Deliver baseline CLI and diagnostics shell
- Gate: parser can process sample modules and output AST

## Milestone M1: Core frontend complete
- Deliver full grammar and AST for baseline constructs
- Deliver module graph, name resolution, and primitive typing
- Deliver structs, functions, control flow, vectors/maps syntax
- Gate: typed AST pass for baseline programs

## Milestone M2: MIR and baseline codegen
- Deliver MIR lowering and baseline LLVM backend
- Deliver runtime allocation boundaries and stdlib baseline
- Deliver conformance suite for Category 1
- Gate: runnable binaries for baseline language subset

## Milestone M3: Ownership and effects core
- Deliver ownership tracking, borrow checker, region semantics
- Deliver effect system with transitive propagation
- Deliver unsafe boundary enforcement and verified unsafe mode
- Gate: compile-fail/pass suites for ownership/effects stable

## Milestone M4: Error system and comptime
- Deliver raise/catch/abort semantics and diagnostics
- Deliver compile-time execution engine and restrictions
- Deliver semantic query tooling and docs
- Gate: all Category 2 guarantees tested

## Milestone M5: Determinism and advanced attributes
- Deliver deterministic checker and scheduling semantics
- Deliver layout transforms, bits structs, ABI contracts
- Deliver SIMD, cache, and tiling attributes
- Gate: deterministic and layout suites green

## Milestone M6: FFI, sandbox, replay, hot reload, patching
- Deliver FFI inference/verification and sandbox runtime
- Deliver recording/replay subsystem with compatibility checks
- Deliver hot reload runtime and semantic patch system
- Gate: full Category 3 and end-to-end guarantees validated

## Milestone M7: Stabilization and release
- Run full fuzzing and performance stabilization
- Freeze diagnostics codes and metadata schemas
- Publish language reference implementation release
- Gate: certification report accepted

# Determinism-Forward Design Rules by Subsystem
## Lexer and parser rules
- Rule: never iterate unordered collections when emitting token or AST dumps
- Rule: canonicalize numeric literal representation at parse time
- Rule: preserve source-order declaration lists without hash-map reordering
- Rule: canonicalize attribute key order in parsed representation
- Rule: deterministic error recovery synchronization priority tables

## Sema rules
- Rule: sort candidate overloads by stable symbol ID before tie-breaking
- Rule: stabilize generic type inference variable numbering
- Rule: deterministic SCC traversal order in call graph analyses
- Rule: deterministic diagnostic emission ordering by source span
- Rule: deterministic effect summary ordering in signatures

## MIR and codegen rules
- Rule: stable block naming and insertion ordering
- Rule: stable temporary naming for debug and IR snapshots
- Rule: stable metadata node numbering
- Rule: deterministic pass order and option normalization
- Rule: canonical emission order for helper runtime stubs

## Runtime rules
- Rule: deterministic allocator mode for deterministic contexts
- Rule: deterministic task merge/join ordering
- Rule: deterministic hash seed control for unordered collections when allowed
- Rule: deterministic serialization for recordings and patch metadata
- Rule: deterministic error payload formatting and ordering

# Detailed Test Matrix
## Category 1 tests
- Lexer tokenization tests for all keywords and punctuators
- Lexer trivia and span mapping tests
- Parser precedence and associativity tests
- Parser recovery multi-error tests
- Name resolution scope and shadow tests
- Primitive type checking and cast tests
- Struct declaration and access tests
- Control flow CFG correctness tests
- Function call arity/type mismatch tests
- Vector API compile and run tests
- Ordered map stable iteration tests
- Unordered map metadata tagging tests
- Reference and pointer syntax tests
- MIR lowering golden tests
- LLVM IR snapshot tests
- Runtime allocation accounting tests
- CLI command integration tests
- Baseline optimization semantic preservation tests

## Category 2 tests
- Ownership move semantics tests
- Borrow conflict and non-lexical lifetime tests
- Region declaration and escape rejection tests
- Effect inference transitivity tests
- effects(none) strict violation tests
- Function type effect compatibility tests
- Unsafe boundary enforcement tests
- Verified unsafe instrumentation tests
- Attribute schema validation tests
- Deterministic basic checker tests
- raise statement control flow tests
- try/catch effect discharge tests
- Unhandled error diagnostics tests
- abort behavior tests
- Catch effect budget tests
- @comp execution and restriction tests
- Compile-time serialization tests
- Semantic query tooling tests
- Optimizer effect barrier tests
- Category 2 performance regression tests

## Category 3 tests
- Deterministic module/function enforcement tests
- Deterministic scheduling output stability tests
- Wall-clock/randomness rejection tests
- Deterministic FP reproducibility tests
- FFI parse and ABI bridge tests
- FFI effect inference and fallback tests
- FFI @verify runtime tests
- Sandbox syscall and memory policy tests
- Structured concurrency lexical join tests
- Spawn capture ownership tests
- Region transfer invalidation tests
- Deterministic concurrency restriction tests
- Layout AoS/SoA transform tests
- layout_of reflection correctness tests
- @transform composition tests
- @bits layout and access tests
- @abi and @verify_layout tests
- Patch generation compatibility tests
- Patch application rollback tests
- Hot reload eligibility tests
- Hot reload function table swap tests
- Hot reload boundary transition tests
- Recorder capture format tests
- Replay compatibility and checksum tests
- SIMD vectorization correctness tests
- Cache alignment layout tests
- Tiling semantics and performance tests
- Deterministic build reproducibility tests
- Full spec certification suite

# Implementation Details Per Feature Group
## Lexer and parser implementation notes
- Use a single-pass lexer with lookahead for multi-character operators
- Keep token enum compact and explicit, avoid string-based token IDs in hot paths
- Parse attributes as first-class nodes attached to declarations and blocks
- Parse effects clause immediately after signature and canonicalize order
- Reserve parser hooks for future grammar evolution with feature flags
- Ensure parser creates nodes even on recoverable errors for richer diagnostics
- Keep parser deterministic by avoiding unordered branch maps

## Type system implementation notes
- Use canonical type interning to reduce memory and enable fast equality
- Store function effects in type key to enforce compatibility accurately
- Represent raise effects as sorted unique list of error type IDs
- Distinguish value categories for move/borrow analysis later
- Treat ptr<T> separately from ref types in safety checks
- Keep region ID optional in type key for non-region references
- Build explicit cast graph rather than ad hoc conversion chains

## MIR implementation notes
- Use explicit basic blocks and terminators for all flow
- Represent raise edges explicitly to integrate with catch lowering
- Attach effect summary hints at instruction and function levels
- Keep origin spans for every MIR instruction for diagnostics and tooling
- Keep deterministic flags on functions and blocks for checker passes
- Use canonical value numbering order for deterministic dumps
- Preserve attribute-derived constraints as MIR annotations

## LLVM backend implementation notes
- Keep all lowering decisions centralized in backend policy modules
- Emit conservative alias metadata unless proven safe by analyses
- Respect effect barriers by using appropriate memory and call attributes
- Avoid backend options that break deterministic builds in deterministic mode
- Emit debug info with normalized paths in reproducible builds
- Keep helper intrinsic declarations in deterministic order
- Emit metadata sections for replay and patching in canonical layout

## Runtime implementation notes
- Runtime allocation APIs must be explicit and auditable
- Region allocator should use bump allocation with clear ownership handles
- Task runtime should expose structured scope objects for joins
- Sandbox runtime should enforce capability-based operation gating
- Replay runtime should deserialize canonical recording format only
- Hot reload runtime should maintain versioned function table and rollback state
- Verification mode should be opt-in and scoped to annotated regions

# Risk Register and Mitigations
## Risk R1: Borrow checker complexity causes schedule slips
- Mitigation: Deliver minimal sound checker first, then improve ergonomics
- Mitigation: Maintain exhaustive negative test corpus from day one
- Mitigation: Use MIR normalization to simplify checker input

## Risk R2: Effect inference unsoundness at FFI boundary
- Mitigation: Use pessimistic fallback by default
- Mitigation: Require explicit @verify or unsafe override for narrowed effects
- Mitigation: Add runtime probes for verification mode

## Risk R3: Deterministic replay drift across targets
- Mitigation: Define supported deterministic target matrix explicitly
- Mitigation: Gate deterministic mode on strict FP and ABI constraints
- Mitigation: Include environment signatures in recording metadata

## Risk R4: Hot reload corrupts long-running state
- Mitigation: Enforce boundary-only swaps and strict eligibility checks
- Mitigation: Keep rollback snapshots and two-phase activation
- Mitigation: Add stress tests with concurrent calls and reloads

## Risk R5: Layout transforms interact badly with borrow rules
- Mitigation: Lower transforms before borrow analysis or provide mapping layer
- Mitigation: Preserve field provenance metadata through transforms
- Mitigation: Add targeted borrow tests for transformed access patterns

## Risk R6: Concurrency semantics difficult to keep deterministic
- Mitigation: Define deterministic join order independent of scheduler timing
- Mitigation: Ban locks and nondeterministic primitives in deterministic contexts
- Mitigation: Add randomized scheduler stress tests verifying stable outputs

# Engineering Process Plan
## Branching and integration
- Use trunk-based development with short-lived feature branches
- Require checker-heavy features to merge with compile-fail tests
- Protect main with conformance, determinism, and replay gates

## Code review standards
- Require guarantee mapping section in every major PR
- Require diagnostics examples for new compile-time checks
- Require benchmark deltas for performance-sensitive changes

## Documentation standards
- Keep language spec examples mirrored in conformance tests
- Publish effect and determinism docs for every stdlib API
- Update migration notes when semantics or diagnostics change

## Release management
- Cut milestone branches at each category gate
- Run full reproducibility audit before release candidates
- Publish known limitations and unsupported target lists explicitly

# Work Breakdown Index
## Category 1 index
- C1-WP01 Source manager and file identity
- C1-WP02 Token definitions and lexical grammar
- C1-WP03 Lexer architecture and trivia model
- C1-WP04 Numeric literal parsing baseline
- C1-WP05 Parser framework and recovery strategy
- C1-WP06 AST schema and node ownership
- C1-WP07 Module graph and import resolution
- C1-WP08 Name binding and symbol table core
- C1-WP09 Baseline type system foundations
- C1-WP10 Built-in primitive types
- C1-WP11 Struct declarations and field access
- C1-WP12 Control flow statements
- C1-WP13 Expression semantics baseline
- C1-WP14 Function declarations and calls
- C1-WP15 Baseline collections: vector type
- C1-WP16 Baseline collections: ordered map type
- C1-WP17 Baseline collections: unordered map type
- C1-WP18 References syntax and baseline typing
- C1-WP19 Raw pointers syntax and baseline typing
- C1-WP20 Pattern of deterministic evaluation order
- C1-WP21 MIR design baseline
- C1-WP22 MIR lowering for control flow
- C1-WP23 MIR lowering for data structures
- C1-WP24 LLVM type mapping baseline
- C1-WP25 LLVM codegen for functions and calls
- C1-WP26 LLVM codegen for control flow and loops
- C1-WP27 Runtime baseline: allocation API boundaries
- C1-WP28 Runtime baseline: string and slice operations
- C1-WP29 Diagnostics framework
- C1-WP30 CLI and build pipeline baseline
- C1-WP31 Baseline optimizer policy
- C1-WP32 Determinism hooks in baseline backend
- C1-WP33 Baseline language conformance tests
- C1-WP34 Standard library baseline APIs
- C1-WP35 Performance baseline and profiling harness
- C1-WP36 Category 1 release gate

## Category 2 index
- C2-WP01 Ownership model formalization in sema
- C2-WP02 Borrow checker constraint graph
- C2-WP03 Region-annotated reference model
- C2-WP04 Region allocation semantics
- C2-WP05 Region escape analysis
- C2-WP06 Effect system core representation
- C2-WP07 Effect inference and propagation pass
- C2-WP08 effects(none) strict verifier
- C2-WP09 Effect-aware function type checking
- C2-WP10 Unsafe block parser and semantic boundaries
- C2-WP11 Verified unsafe mode instrumentation
- C2-WP12 Basic attribute framework
- C2-WP13 @deterministic basic checker hooks
- C2-WP14 @opt attribute baseline behavior
- C2-WP15 Error type declarations and hierarchy
- C2-WP16 raise statement semantics
- C2-WP17 try/catch semantics and effect discharge
- C2-WP18 Unhandled error propagation diagnostics
- C2-WP19 abort semantics and runtime behavior
- C2-WP20 Effect constraints inside catch blocks
- C2-WP21 Compile-time execution engine architecture
- C2-WP22 @comp restriction enforcement
- C2-WP23 Compile-time value serialization
- C2-WP24 Borrow checker diagnostics quality pass
- C2-WP25 Region and borrow MIR annotations
- C2-WP26 Basic attribute set completion
- C2-WP27 Effect summaries for stdlib
- C2-WP28 Effect-aware optimization constraints
- C2-WP29 Deterministic preliminary floating-point policy
- C2-WP30 Semantic query tooling
- C2-WP31 Verification mode runtime library
- C2-WP32 Category 2 conformance suite
- C2-WP33 Category 2 performance and scalability
- C2-WP34 Category 2 release gate

## Category 3 index
- C3-WP01 Full determinism checker architecture
- C3-WP02 Deterministic module-level enforcement
- C3-WP03 Deterministic scheduling contract
- C3-WP04 Wall-clock and randomness gating
- C3-WP05 Deterministic floating-point complete policy
- C3-WP06 FFI declaration parser and type bridge
- C3-WP07 FFI static effect inference engine
- C3-WP08 FFI pessimistic fallback behavior
- C3-WP09 FFI explicit effect annotation verification
- C3-WP10 Sandbox runtime core
- C3-WP11 Sandbox compiler integration
- C3-WP12 Structured concurrency runtime
- C3-WP13 Concurrency capture and ownership checks
- C3-WP14 Region transfer in spawn
- C3-WP15 Deterministic concurrency restrictions
- C3-WP16 Data layout attribute core
- C3-WP17 SoA transformation lowering
- C3-WP18 Layout reflection API
- C3-WP19 @transform attribute implementation
- C3-WP20 @bits struct implementation
- C3-WP21 ABI contracts and @abi(C)
- C3-WP22 @verify_layout implementation
- C3-WP23 Semantic binary patch metadata emission
- C3-WP24 Patch generation command implementation
- C3-WP25 Patch application runtime
- C3-WP26 Hot reload eligibility checker
- C3-WP27 Hot reload function table runtime
- C3-WP28 Hot reload boundary semantics
- C3-WP29 Hot reload error compatibility
- C3-WP30 Recording API implementation
- C3-WP31 Replay engine implementation
- C3-WP32 Replay environment compatibility checks
- C3-WP33 Deterministic checksum model
- C3-WP34 Complex attribute conflict system
- C3-WP35 SIMD attribute lowering
- C3-WP36 Cache alignment attribute lowering
- C3-WP37 Tiling construct implementation
- C3-WP38 Advanced optimizer with contracts
- C3-WP39 Deterministic build reproducibility system
- C3-WP40 Final conformance and certification gate

# Detailed Sequence Plan (Quarter-by-Quarter)
## Q1
- Build source manager, lexer, parser, AST
- Build diagnostics and module graph
- Build primitive typing and name resolution
- Build initial MIR and dumps
- Deliver end-of-quarter demo compiling simple functions

## Q2
- Complete structs, control flow, and baseline collections
- Complete LLVM baseline codegen and runtime allocation APIs
- Build Category 1 conformance suite and CI gates
- Stabilize CLI and developer workflow tools
- Deliver Category 1 freeze

## Q3
- Build ownership and borrow checker
- Build region declarations and escape analysis
- Build effect representation, inference, and enforcement
- Build unsafe blocks and verified unsafe instrumentation
- Deliver ownership/effects compile-fail quality bar

## Q4
- Build error handling (raise/catch/abort)
- Build compile-time execution engine and restrictions
- Build semantic query tooling
- Harden diagnostics and pass performance
- Deliver Category 2 freeze

## Q5
- Build deterministic checker complete and deterministic runtime policies
- Build concurrency runtime and deterministic concurrency rules
- Build layout attributes, SoA, reflection, and bit structs
- Build ABI and verify_layout checks
- Deliver deterministic + layout alpha

## Q6
- Build FFI inference, fallback, and verification
- Build sandbox runtime and language integration
- Build recording/replay subsystem
- Build hot reload runtime and checks
- Build semantic patch generation and apply runtime
- Deliver Category 3 freeze and certification run

# Guarantee Mapping Table (Narrative)
## Safety by construction
- Enforced by ownership checker, borrow constraints, region checks, and unsafe boundaries
- Verified by compile-fail suites and stress tests
- Extended by verified unsafe instrumentation when enabled

## Explicit effects
- Enforced by effect type integration and transitive propagation
- Verified by function signature checks and call graph closure checks
- Surfaced by tooling queries and stdlib audits

## Deterministic semantics
- Enforced by deterministic checker and runtime constraints
- Verified by replay and reproducibility test suites
- Guarded by deterministic FP, ordering, and scheduling policies

## Zero-cost abstractions
- Enforced by MIR-to-LLVM lowering without hidden control paths
- Verified by IR inspections and performance regressions checks
- Strengthened by compile-time layout transforms and intrinsic boundaries

## Data-oriented defaults
- Enforced by layout attributes and transform pipeline
- Verified by layout reflection correctness and performance profiles
- Integrated with cache and SIMD features

# Open Design Decisions and Finalization Steps
## Decision D1: generic system details
- Finalize generic constraints before deep stdlib expansion
- Ensure generic instantiation deterministic ordering policy
- Define monomorphization cache keys including effect signatures

## Decision D2: deterministic target matrix
- Decide exact architecture/OS combos guaranteed for bitwise deterministic replay
- Publish unsupported combinations and fallback behavior
- Lock this matrix before replay beta

## Decision D3: sandbox policy backend
- Decide whether to use seccomp-like model, capability model, or hybrid by platform
- Provide portable abstraction with target-specific enforcement modules
- Validate policy portability early

## Decision D4: patch format
- Finalize patch container format and signature scheme
- Define compatibility versioning and deprecation policy
- Build tooling for offline verification and auditing

## Decision D5: hot reload activation model
- Decide sync points granularity for activation beyond function boundaries
- Validate latency and safety trade-offs with real workloads
- Publish precise behavior for long-running loops

# Operational Checklist (Always-On)
- Keep deterministic metadata schema versioned and migration-tested
- Keep every checker pass side-effect free for reproducibility
- Keep conformance tests in sync with language spec revisions
- Keep CI running with both deterministic and non-deterministic modes
- Keep nightly fuzzing for parser and sema frontends
- Keep sanitizer builds for runtime safety components
- Keep benchmark suite fixed-input and version-pinned
- Keep release artifacts reproducibility-audited

# Completion Definition
- All Category 1 work packages complete and passing gates
- All Category 2 work packages complete and passing gates
- All Category 3 work packages complete and passing gates
- Every spec guarantee mapped to automated tests and passing
- Deterministic build and replay guarantees validated on supported matrix
- Hot reload, patching, and sandbox systems validated under stress
- Final certification report generated and archived

# Appendix A: Per-Pass Inputs and Outputs
## Lexer pass
- Inputs: source bytes and file metadata
- Outputs: token stream, trivia stream, lexical diagnostics
- Determinism note: token order strictly source order

## Parser pass
- Inputs: token stream and trivia
- Outputs: AST and syntax diagnostics
- Determinism note: parser recovery strategy deterministic by fixed priority sets

## Name resolution pass
- Inputs: AST and module graph
- Outputs: bound symbols and resolution diagnostics
- Determinism note: scope traversal in source order

## Type checking pass
- Inputs: bound AST and type environment
- Outputs: typed AST and type diagnostics
- Determinism note: inference variable assignment stable by node ID order

## MIR lowering pass
- Inputs: typed AST
- Outputs: MIR and lowering diagnostics
- Determinism note: block IDs assigned in lexical lowering order

## Borrow and region pass
- Inputs: MIR and type metadata
- Outputs: borrow facts, region checks, diagnostics
- Determinism note: fixed-point iteration ordered by block ID

## Effect pass
- Inputs: MIR, call graph, intrinsic summaries
- Outputs: effect summaries, diagnostics
- Determinism note: SCC order canonicalized by function ID

## Determinism pass
- Inputs: MIR, effect summaries, attribute metadata
- Outputs: deterministic eligibility map, diagnostics
- Determinism note: deterministic pass itself deterministic by design

## Compile-time execution pass
- Inputs: MIR subset and @comp declarations
- Outputs: constant values and diagnostics
- Determinism note: interpreter seeded with fixed deterministic context

## Codegen pass
- Inputs: MIR and all semantic annotations
- Outputs: LLVM IR, object files, metadata artifacts
- Determinism note: emission order canonicalized by symbol ID

# Appendix B: Example Policy Defaults
- Default map type in deterministic contexts should be ordered map
- Unordered map allowed only outside deterministic contexts unless deterministic hash mode enabled
- Default spawn join ordering in deterministic mode is lexical spawn order
- Default @verify behavior is off unless explicitly requested
- Default FFI effect behavior is pessimistic fallback when unknown
- Default hot reload strict mode rejects incompatible raise-set growth

# Appendix C: Tooling Commands to Deliver
- sable check for compile-time semantic validation only
- sable build for full compile and link
- sable test for conformance and runtime tests
- sable dump tokens for lexer debugging
- sable dump ast for parser and syntax inspection
- sable dump mir for semantic and borrow/effect debugging
- sable dump ir for backend verification
- sable query effects for effect graph inspection
- sable query errors for raise propagation analysis
- sable replay capture and replay commands for deterministic modules
- sable hot reload apply for runtime swap
- sable build --patch-from for semantic patch artifacts

# Final Notes
- This plan intentionally introduces determinism hooks before full determinism rollout
- This plan keeps effect and safety metadata attached from parser to backend
- This plan prioritizes soundness before ergonomics in checker implementation
- This plan ensures advanced features reuse core semantics instead of bypassing them
- This plan is designed to match the explicit guarantees and restrictions in the provided Sable spec
