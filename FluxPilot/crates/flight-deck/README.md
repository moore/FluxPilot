# FlightDeck

FlightDeck is the UI for FluxPilot. It consistes of a Rust library compiled to wasm and a UI in HTML + JavaScript. It talks to the FluxPilot board over WebUSB using a custome protocol. The Rust code is responsible for creating and decoding messages in the wire protocol and the Js handles talking to the USB apis and interacting with the user.

## Rust APIs

### Call Function

The wire protocol supports RCPs to functions of FluxPilot Machaines. They consist of a machine id, and function id and a list of arguments. The arguments are pushed onto the stack and the spisifined fucntino is called.

```rust
fn call( machine: MachineId, function: FunctionId, args: &[StackValue]) -> Result<Vec<StackValue, MAX_RESULT>, ProtocoErrorType>;
```

If an error is encountered calling or running the function a `ErrorType` is return, otherwise any values remaining on the stack after the called function returns are provided to the caller.

### Load Program

A compiled program is sent to the hardware and actavated.

```rust
fn load_program(program: &[ProgramValue]) -> Result<(), ErrorType>
```

### Building a program
