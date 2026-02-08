# I2C Integration Specification (Firmware)

## Status

Draft.

## Terminology

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD",
"SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" are to be interpreted as
described in RFC 2119 and RFC 8174.

## Overview

This document specifies the firmware behavior for handling I2C messages,
dispatching them into the Light Machine VM, and routing messages over USB to
the UI for mapping and configuration.

## Design Rationale (Non-Normative)

The firmware is intentionally generic: it does not parse device-specific I2C
payloads. Instead, it wraps raw I2C fields into a structured event and hands
them to the VM or UI. This keeps firmware stable while allowing device logic to
evolve in the VM program.

Routing is owned by the VM program, not firmware configuration. This keeps
behavior consistent with the loaded program and allows UIs to update mappings
through VM static functions.

Single-message VM dispatch keeps the VM API simple and deterministic. A ring
buffer absorbs bursts and handles overruns overwriting old messages.

USB routing mode exists to support interactive mapping workflows without
requiring a custom firmware build.

## Data Model

### I2C Event

An I2C event is a structured wrapper for raw I2C fields. Unless otherwise
noted, all fields are serialized as VM stack words (`u32`).

- `bus_id` (u8)
- `address_7bit` (u8)
- `is_read` (bool, 0 or 1)
- `payload` (bytes)
- `timestamp_us` (u32)

## Requirements

### Event Capture and Queueing

- I2C-FW-REQ-001: Firmware MUST wrap every I2C message into an I2C Event.
- I2C-FW-REQ-002: Firmware MUST process one I2C Event at a time.
- I2C-FW-REQ-003: Firmware MUST enqueue additional I2C Events in a fixed-size
  ring buffer.
- I2C-FW-REQ-004: If the ring buffer is full, firmware MUST overwrite the
  oldest entry and MUST increment an internal drop counter.

### VM Dispatch

- I2C-FW-REQ-010: Firmware MUST dispatch I2C Events to the VM via
  `vm_handle_i2c(event: I2CEvent)`.
- I2C-FW-REQ-011: Routing MUST be keyed by `(bus_id, address_7bit)`.
- I2C-FW-REQ-012: Each route entry MUST contain a list of
  `(machine_id, function_id)` targets.
- I2C-FW-REQ-013: If no mapping exists for an event, firmware MUST drop the
  message.

### Routing Table Source

- I2C-FW-REQ-020: The routing table MUST be provided by the loaded VM program.
- I2C-FW-REQ-021: Firmware MUST call the static function `get_routes` (id `1`)
  to load or refresh the routing table.
- I2C-FW-REQ-022: After calling `add_route` or `remove_route`, firmware MUST
  reload the routing table.

### Static Function IDs

- I2C-FW-REQ-030: Static function id `0` MUST be reserved for `init_program`.
- I2C-FW-REQ-031: Static function id `1` MUST be reserved for `get_routes`.
- I2C-FW-REQ-032: Static function id `2` MUST be reserved for `add_route`.
- I2C-FW-REQ-033: Static function id `3` MUST be reserved for `remove_route`.

### Static Function Stack Conventions

- I2C-FW-REQ-040: `get_routes` MUST return the routing table on the stack using
  the encoding defined in "Routing Table Stack Encoding".
- I2C-FW-REQ-041: `add_route` MUST accept arguments on the stack in this order:
  `bus_id`, `address_7bit`, `machine_id`, `function_id`.
- I2C-FW-REQ-042: `remove_route` MUST accept arguments on the stack in this
  order: `bus_id`, `address_7bit`, `machine_id`, `function_id`.

### Routing Table Stack Encoding

The routing table is returned as a flat list of `u32` stack values:

```
entry_count,
  bus_id, address_7bit, target_count,
    machine_id, function_id, ... (target_count pairs)
  ... (repeat for entry_count entries)
```

- I2C-FW-REQ-050: `entry_count` MUST be the number of routing entries.
- I2C-FW-REQ-051: Each `target_count` MUST be the number of
  `(machine_id, function_id)` pairs that follow for that entry.

### USB Routing Mode

- I2C-FW-REQ-060: Firmware MUST support two routing modes:
  `I2C_ROUTE_VM` and `I2C_ROUTE_USB`.
- I2C-FW-REQ-061: In `I2C_ROUTE_VM`, firmware MUST dispatch events to the VM.
- I2C-FW-REQ-062: In `I2C_ROUTE_USB`, firmware MUST send I2C events to the UI
  and MUST NOT dispatch them to the VM.
- I2C-FW-REQ-063: In `I2C_ROUTE_USB`, the `I2C_EVENT` message MUST include
  `bus_id`, `address_7bit`, `is_read`, and `payload`.
- I2C-FW-REQ-064: In `I2C_ROUTE_USB`, the `I2C_EVENT` message MUST omit
  `timestamp_us`.

### USB Protocol

- I2C-FW-REQ-070: Firmware MUST implement `CALL_STATIC_FUNCTION` messages to
  invoke VM static functions by id (no machine id).
- I2C-FW-REQ-071: Firmware MUST implement `STATIC_FUNCTION_RESULT` messages
  that include a request id and return either a result stack or an error code.
