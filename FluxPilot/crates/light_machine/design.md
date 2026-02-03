# light_machine VM Specification

This document defines the `light_machine` virtual machine, its memory layout,
stack model, calling convention, and opcode semantics. It is the authoritative
reference for code generation, assemblers, and runtime behavior.

## Word size and types

- `ProgramWord` is `u16` (unsigned 16-bit).
- `StackWord` is `u32` (unsigned 32-bit).
- All instruction words and data words are `ProgramWord`.
- Stack elements are `StackWord`.
- Machine indices, function indices, and program addresses are `ProgramWord`
  values that are interpreted as `usize` for indexing when safe.

## Program memory layout

Programs are stored in a single contiguous `ProgramWord` array. The runtime is given:

- `static_data`: the program `ProgramWord` array.
- `globals`: a mutable `ProgramWord` array for persistent per-program state.

The program header (versioned):

```
[0] VERSION
[1] MACHINE_COUNT (instance count)
[2] GLOBALS_SIZE (total globals required for all instances, including shared globals)
[3] SHARED_FUNCTION_COUNT
[4] TYPE_COUNT
[5] INSTANCE_TABLE_OFFSET
[6] TYPE_TABLE_OFFSET
[7] SHARED_FUNCTION_TABLE_OFFSET
```

The instance table entries point to a machine type and a globals base.
The type table entries point to a function table for that type.
Shared function table entries point to program-scoped shared function entry points.
See `FluxPilot/crates/light_machine/shared_functions_plan.md` for the detailed
shared function semantics.

## Program graph emission

Compilers may build a program graph of static data, functions, machine types,
and machine instances before emitting a program. The graph is used to
deduplicate identical definitions (statics, functions, and types) while keeping
instance ordering stable. Emission still produces the same runtime tables
(instances, types, shared functions); the graph only affects how entries are
shared and the final program layout.

Implementation notes (current):

- The graph is built in `flight-deck` and emitted into the existing
  `ProgramBuilder`.
- Function bodies are stored as word references (literals, static refs, and
  label offsets). Label offsets are resolved to absolute addresses at emit time.
- Both per-machine static data and shared static data are emitted into the
  shared static region so all static addresses are global within `static_data`.

## Instance + type tables

Instance table layout (at `INSTANCE_TABLE_OFFSET`):

```
For each instance `i`:
  [0] TYPE_ID          ; index into the type table
  [1] GLOBALS_BASE     ; offset into the globals buffer
```

Type table layout (at `TYPE_TABLE_OFFSET`):

For each type `t`:
  [0] FUNCTION_COUNT
  [1] FUNCTION_TABLE_OFFSET

Function index convention: `0 = init`, `1 = get_color`, remaining are user-defined.

Function tables (pointed to by `FUNCTION_TABLE_OFFSET`) are sequences of entry
points into `static_data`:

  [entry_point_0, entry_point_1, ... entry_point_n]

## Stack model

- The VM is stack-based. The stack holds `StackWord` values.
- Most operations pop their operands from the stack and push results.
- Stack underflow/overflow are runtime errors.

### Frame pointer

The VM maintains a `frame_pointer` that indexes into the current stack.
`SLOAD` and `SSTORE` use offsets relative to the frame pointer.

`frame_pointer` is a `StackWord` and is interpreted as a `usize` index (checked).

### Machine locals pointer

The VM maintains a machine locals base pointer (`mlp`) that is set on entry to a
machine (and shared functions called from that machine). `LLOAD` and `LSTORE`
use `mlp + offset` to access machine-local globals.

## Calling convention

`CALL` expects the stack to contain:

```
... arg0 arg1 ... argN-1 arg_count func_index
```

Semantics:

1. Pop `func_index`.
2. Pop `arg_count`.
3. Compute `arg_start = stack_len - arg_count`.
4. Insert `return_pc` at `arg_start`.
5. Insert `saved_frame_pointer` at `arg_start + 1`.
6. Set `frame_pointer = arg_start + 2` (points to `arg0`).
7. Jump to the function entry point.
8. On return, restore `frame_pointer` to the saved value.

`RET <count>` uses the current `frame_pointer` to locate the saved `return_pc`
and saved frame pointer. It copies `<count>` values from the top of the stack,
removes the call frame, restores `frame_pointer`, pushes the copied values, and
sets the program counter to `return_pc`.

## Instruction encoding

- Each instruction is a single `ProgramWord` opcode.
- Some opcodes are followed by a single `ProgramWord` immediate operand.
- For convenience, the assembler expands `CALL`/`JUMP`/branches with operands
  into `PUSH <operand>` + `<op>`. For `CALL`, the caller must still push
  `arg_count` beneath the function index.

## Opcode table

The following opcodes are implemented. For all operations, stack underflow or
invalid indices are runtime errors.

### Stack and data ops

- `PUSH <word>`: push immediate.
- `POP`: pop and discard top.
- `DUP`: duplicate top.
- `SWAP`: swap top two values.
- `SLOAD <offset>`: push `stack[frame_pointer + offset]`.
- `SSTORE <offset>`: store top at `stack[frame_pointer + offset]`, then pop top.
- `LLOAD <offset>`: push `globals[mlp + offset]`.
- `LSTORE <offset>`: pop and store to `globals[mlp + offset]`.
- `GLOAD <addr>`: push `globals[addr]`.
- `GSTORE <addr>`: pop and store to `globals[addr]`.
- `LOAD_STATIC`: pop `addr`, push `static_data[addr]`.

### Control flow

- `JUMP`: pop `addr`, set `pc = addr`.
- `CALL`: pop `func_index` and `arg_count`, build call frame, jump to function.
- `RET <count>`: restore frame pointer, return `<count>` values to the caller,
  jump to saved return PC (does not unwind the host call to `run`).
- `BRLT`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs < rhs`, jump to `addr`.
- `BRLTE`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs <= rhs`, jump to `addr`.
- `BRGT`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs > rhs`, jump to `addr`.
- `BRGTE`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs >= rhs`, jump to `addr`.
- `BREQ`: pop `addr`, pop `lhs`, pop `rhs`; if `lhs == rhs`, jump to `addr`.
- `EXIT`: return from the current `run` invocation without frame handling.

### Logical ops

- `AND`: pop `lhs`, `rhs`; push `1` if both non-zero else `0`.
- `OR`: pop `lhs`, `rhs`; push `1` if either non-zero else `0`.
- `XOR`: pop `lhs`, `rhs`; push `1` if exactly one is non-zero else `0`.
- `NOT`: pop `value`; push `1` if zero else `0`.

### Bitwise ops

- `BAND`: pop `lhs`, `rhs`; push `lhs & rhs`.
- `BOR`: pop `lhs`, `rhs`; push `lhs | rhs`.
- `BXOR`: pop `lhs`, `rhs`; push `lhs ^ rhs`.
- `BNOT`: pop `value`; push `!value`.

### Arithmetic ops

- `ADD`: pop `lhs`, `rhs`; push `lhs + rhs` (wrapping).
- `SUB`: pop `lhs`, `rhs`; push `lhs - rhs` (wrapping).
- `MUL`: pop `lhs`, `rhs`; push `lhs * rhs` (wrapping).
- `DIV`: pop `lhs`, `rhs`; push `lhs / rhs` (error on division by zero).
- `MOD`: pop `lhs`, `rhs`; push `lhs % rhs` (error on division by zero).

## Runtime error conditions

The VM reports errors for:

- Invalid opcode word.
- Stack underflow/overflow.
- Out-of-bounds `globals` access.
- Out-of-bounds `static_data` access.
- Division/modulo by zero.

## Notes

- `EXIT` is a simple return from the VM that does not unwind the call frame. Use `RET <count>`
  for functions using the frame-pointer calling convention.
- Any entry points to the VM must end in `EXIT` as the inital stack
  has no frame pointer or return address below the frame pointer.
- The assembler is case-insensitive for mnemonics.
