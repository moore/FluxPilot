use super::*;
extern crate std;
use std::dbg;

#[test]
fn test_new_builder() -> Result<(), MachineBuilderError> {
    let mut buffer = [0u16; 1024];
    let machine_count = 3;
    let _ = ProgramBuilder::new(&mut buffer, machine_count)?;
    assert_eq!(0, buffer[0]);
    Ok(())
}

#[test]
fn test_new_machine_builder() -> Result<(), MachineBuilderError> {
    let machine_count = 3;
    let function_count = 5;
    #[rustfmt::skip]
    let goal= [
        2u16,           // machine count ( one allocated but not used)
        0,              // Globals size
        5, 15, 0,       // machine pointers (last unused)
        0,              // globals size
        0,              // globals offset
        0, 0, 0, 0, 0,  // Machine 1 function table (no functions)
        17, 31, 71,     // Machine 1 static data
        0,              // Globals size
        0,              // globals offset
        0, 0, 0, 0, 0,  // Machine 2 fucntion table (no functions)
        7, 11, 97,      // Manchie 2 static data
    ];

    let mut buffer = [0u16; 25];
    let program = ProgramBuilder::new(&mut buffer, machine_count)?;

    let mut machine = program.new_machine(function_count, 0)?;
    let data = [17, 31, 71];
    let index = machine.add_static(data.as_slice())?;
    let program = machine.finish();

    let mut machine = program.new_machine(function_count, 0)?;
    let data = [7, 11, 97];
    let index = machine.add_static(data.as_slice())?;
    let program = machine.finish();

    assert_eq!(buffer, goal);
    Ok(())
}

#[test]
fn test_new_function_builder() -> Result<(), MachineBuilderError> {
    let machine_count = 1;
    let function_count = 2;
    let globals_size = 2;

    #[rustfmt::skip]
    let goal = [
        1u16,             // machine count
        2,                // Globals Size
        3,                // machine pointer
        2,                // Globals size
        0,                // Globals offset
        17, 10,           // Machine 1 function table
        17, 31, 71,       // Machine 1 static data
        Ops::Push.into(), // Function 1
        11,
        Ops::Load.into(),
        0,
        Ops::Store.into(),
        1,
        Ops::Return.into(),
        Ops::Load.into(),   // Function 2
        1,
        Ops::Return.into(),
    ];

    let mut buffer = [0u16; 20];
    let program = ProgramBuilder::new(&mut buffer, machine_count)?;

    let mut machine = program.new_machine(function_count, globals_size)?;
    let data = [17, 31, 71];
    let index = machine.add_static(data.as_slice())?;
    let funtion_index = machine.reserve_function()?;

    let mut function = machine.new_function()?;
    function.add_op(Op::Push(11))?;
    function.add_op(Op::Load(0))?;
    function.add_op(Op::Store(1))?;
    function.add_op(Op::Return)?;
    let (fn_index, machine) = function.finish();

    let mut function = machine.new_function_at_index(funtion_index)?;
    function.add_op(Op::Load(1))?;
    function.add_op(Op::Return)?;
    let (fn_index, machine) = function.finish();

    let program: ProgramBuilder<'_> = machine.finish();

    assert_eq!(buffer, goal);
    Ok(())
}
