use super::*;
extern crate std;

#[test]
fn test_new_builder() -> Result<(), MachineBuilderError> {
    let mut buffer = [0u16; 1024];
    const MACHINE_COUNT: usize = 3;
    const FUNCTION_COUNT: usize = 5;
    let _ = ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(
        &mut buffer,
        MACHINE_COUNT as u16,
        MACHINE_COUNT as u16,
        0,
    )?;
    assert_eq!(PROGRAM_VERSION, buffer[0]);
    Ok(())
}

#[test]
fn test_new_machine_builder() -> Result<(), MachineBuilderError> {
    const MACHINE_COUNT: usize = 3;
    const FUNCTION_COUNT: usize = 5;
    let mut buffer = [0u16; 64];
    let program = ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(
        &mut buffer,
        MACHINE_COUNT as u16,
        MACHINE_COUNT as u16,
        0,
    )?;

    let mut machine = program.new_machine(FUNCTION_COUNT as u16, 0)?;
    let data = [17, 31, 71];
    let _index = machine.add_static(data.as_slice())?;
    let program = machine.finish()?;

    let mut machine = program.new_machine(FUNCTION_COUNT as u16, 0)?;
    let data = [7, 11, 97];
    let _index = machine.add_static(data.as_slice())?;
    let _program = machine.finish();

    assert_eq!(buffer[VERSION_OFFSET], PROGRAM_VERSION);
    assert_eq!(buffer[MACHINE_COUNT_OFFSET], 3);
    assert_eq!(buffer[TYPE_COUNT_OFFSET], 3);
    assert_eq!(buffer[SHARED_FUNCTION_COUNT_OFFSET], 0);
    assert_eq!(buffer[INSTANCE_TABLE_OFFSET], HEADER_WORDS as u16);
    assert_eq!(buffer[TYPE_TABLE_OFFSET], (HEADER_WORDS + 6) as u16);
    assert_eq!(
        buffer[SHARED_FUNCTION_TABLE_OFFSET],
        (HEADER_WORDS + 12) as u16
    );
    let instance_table = HEADER_WORDS;
    assert_eq!(buffer[instance_table], 0);
    assert_eq!(buffer[instance_table + 1], 0);
    assert_eq!(buffer[instance_table + 2], 1);
    assert_eq!(buffer[instance_table + 3], 0);
    let type_table = HEADER_WORDS + 6;
    assert_eq!(buffer[type_table], FUNCTION_COUNT as u16);
    assert_eq!(buffer[type_table + 1], (HEADER_WORDS + 12) as u16);
    assert_eq!(buffer[type_table + 2], FUNCTION_COUNT as u16);
    assert_eq!(buffer[type_table + 3], (HEADER_WORDS + 20) as u16);
    assert_eq!(buffer[HEADER_WORDS + 12 + 5], 17);
    assert_eq!(buffer[HEADER_WORDS + 12 + 6], 31);
    assert_eq!(buffer[HEADER_WORDS + 12 + 7], 71);
    assert_eq!(buffer[HEADER_WORDS + 20 + 5], 7);
    assert_eq!(buffer[HEADER_WORDS + 20 + 6], 11);
    assert_eq!(buffer[HEADER_WORDS + 20 + 7], 97);
    Ok(())
}

#[test]
fn test_new_function_builder() -> Result<(), MachineBuilderError> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 2;
    let globals_size = 2;

    let mut buffer = [0u16; 64];
    let program = ProgramBuilder::new(&mut buffer, MACHINE_COUNT as u16, MACHINE_COUNT as u16, 0)?;

    let mut machine = program.new_machine(FUNCTION_COUNT as u16, globals_size)?;
    let data = [17, 31, 71];
    let _index = machine.add_static(data.as_slice())?;
    let funtion_index = machine.reserve_function()?;

    let mut function = machine.new_function()?;
    function.add_op(Op::Push(11))?;
    function.add_op(Op::LocalLoad(0))?;
    function.add_op(Op::LocalStore(1))?;
    function.add_op(Op::Exit)?;
    let (_fn_index, machine) = function.finish()?;

    let mut function = machine.new_function_at_index(funtion_index)?;
    function.add_op(Op::LocalLoad(1))?;
    function.add_op(Op::Exit)?;
    let (_fn_index, machine) = function.finish()?;

    let _program: ProgramBuilder<'_, MACHINE_COUNT, FUNCTION_COUNT> = machine.finish()?;

    assert_eq!(buffer[VERSION_OFFSET], PROGRAM_VERSION);
    assert_eq!(buffer[MACHINE_COUNT_OFFSET], 1);
    assert_eq!(buffer[TYPE_COUNT_OFFSET], 1);
    assert_eq!(buffer[SHARED_FUNCTION_COUNT_OFFSET], 0);
    let instance_table = HEADER_WORDS;
    assert_eq!(buffer[instance_table], 0);
    assert_eq!(buffer[instance_table + 1], 0);
    let type_table = HEADER_WORDS + 2;
    assert_eq!(buffer[type_table], FUNCTION_COUNT as u16);
    assert_eq!(buffer[type_table + 1], (HEADER_WORDS + 4) as u16);
    let static_start = HEADER_WORDS + 6;
    assert_eq!(buffer[static_start], 17);
    assert_eq!(buffer[static_start + 1], 31);
    assert_eq!(buffer[static_start + 2], 71);
    Ok(())
}
