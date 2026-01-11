# Flight Deck Interface Spec

This document defines the intended interface and behavior for the new
`index.html` in `crates/flight-deck/`.

## Goals

- Provide a clean, fast single-page interface for viewing and controlling
  Flight Deck content.
- Support both desktop and mobile layouts without sacrificing clarity.
- Minimize dependencies; keep the page portable and easy to embed.
- The software should be based on web components and not use any frameworks such as
  React or Angular.

## Overall concept

The UI will be inspired by music and animation software. The core concept is that
the program will be represented by a list of machines (see `crates/light_machine/design.md`).

Definitions:

- Machine: a compiled `light_machine` program instance with one or more functions
  (see `crates/light_machine/design.md`).
- Function: a callable VM entry point that may compute LED values from inputs
  (see `crates/light_machine/design.md`).
- Track: a UI lane that references a machine and presents its parameters,
  similar to a tracker row or pattern.

User model:

- Users select a machine for each track.
- Each track represents one machine in the UI, even though the machine inputs
  (tick and LED index) are not exposed directly.

Settings:

- Track settings apply to the selected machine (e.g., speed, color).
- Settings map to machine inputs or globals defined by the assembler program
  (see `crates/light_machine/language.md` for program construction).

## UI data model (ui.js)

`crates/flight-deck/ui.js` defines the UI-facing model used to describe machines,
their functions, and the controls needed to drive them.

Machine descriptors:

- `MachineDescriptor`: describes a machine with an `id`, `name`, `assembly` text,
  a list of function descriptors, and a list of control descriptors.
- `MachineFunctionDescriptor`: describes a callable function (name, description)
  plus how to map control values into a Deck call.
- `MachineControlDescriptor`: describes a single control input (range/select),
  including min/max/step, default value, and the function it invokes.

Behavior:

- `MachineDescriptor.buildDeckCall(functionId, values, machineIndex)` creates a
  `{ machineIndex, functionIndex, args }` payload suitable for invoking `deck.js`
  with the current control values.
- `MachineDescriptor.buildDeckCallForControl(controlId, values, machineIndex)`
  uses the control’s function mapping to build the call payload.
- Default control values are derived from the machine’s controls list to keep UI
  and call wiring in sync.

Defaults:

- `DEFAULT_MACHINE_RACK` provides the initial machine list for the rack, including
  the Assembler Program and example function/control metadata.
