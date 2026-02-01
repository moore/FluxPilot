# light_machine VM Specification

This document defines the `light_machine` virtual machine, its memory layout,
stack model, calling convention, and opcode semantics. It is the authoritative
reference for code generation, assemblers, and runtime behavior.

## Word size and types

- `Word` is `u16` (unsigned 16-bit).
- All instruction words and data words are `Word`.
- Stack elements are `Word`.
- Machine indices, function indices, and addresses are `Word` values that are
  interpreted as `usize` for indexing when safe.

## Program memory layout

Programs are stored in a single contiguous `Word` array. The runtime is given:

- `static_data`: the program `Word` array.
- `globals`: a mutable `Word` array for persistent per-program state.

The program header:

```
[0] MACHINE_COUNT
[1] GLOBALS_SIZE (total globals required for all machines)
[2..] MACHINE_TABLE (machine_count entries)
```

Each entry in the machine table points to the start of a machine block.

## Machine block layout

Each machine block is laid out as:

```
machine_start:
  [0] GLOBALS_SIZE     ; globals required by this machine
  [1] GLOBALS_OFFSET   ; offset into the globals buffer
  [2..] FUNCTION_TABLE (function_count entries)
  [...] STATIC_DATA (machine-specific data section)
  [...] FUNCTION_BODIES
```

Function table entries hold word offsets into `static_data`. The function
count is not stored in the program image; it is known to the builder/host.

## Stack model

- The VM is stack-based. The stack holds `Word` values.
- Most operations pop their operands from the stack and push results.
- Stack underflow/overflow are runtime errors.

### Frame pointer

The VM maintains a `frame_pointer` that indexes into the current stack.
`SLOAD` and `SSTORE` use offsets relative to the frame pointer.

`frame_pointer` is a `Word` and is interpreted as a `usize` index.

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

- Each instruction is a single `Word` opcode.
- Some opcodes are followed by a single `Word` immediate operand.
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
- `LOAD <addr>`: push `globals[addr]`.
- `STORE <addr>`: pop and store to `globals[addr]`.
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
