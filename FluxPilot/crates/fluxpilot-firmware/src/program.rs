use light_machine::builder::{MachineBuilderError, Op, ProgramBuilder};

#[link_section = ".coldtext"]
#[inline(never)]
pub fn default_program(buffer: &mut [u16]) -> Result<usize, MachineBuilderError> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 3;
    let program_builder =
        ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(buffer, MACHINE_COUNT as u16, 0)?;

    let globals_size = 3;
    let machine = program_builder.new_machine(FUNCTION_COUNT as u16, globals_size)?;
    let mut function = machine.new_function()?;
    function.add_op(Op::Push(0))?;
    function.add_op(Op::LocalStore(0))?;
    function.add_op(Op::Push(16))?;
    function.add_op(Op::LocalStore(1))?;
    function.add_op(Op::Push(8))?;
    function.add_op(Op::LocalStore(2))?;
    function.add_op(Op::Exit)?;
    let (_function_index, machine) = function.finish()?;

    let mut function = machine.new_function()?;
    function.add_op(Op::LocalLoad(0))?;
    function.add_op(Op::LocalLoad(1))?;
    function.add_op(Op::LocalLoad(2))?;
    function.add_op(Op::Exit)?;
    let (_function_index, machine) = function.finish()?;

    let mut function = machine.new_function()?;
    function.add_op(Op::LocalStore(0))?;
    function.add_op(Op::LocalStore(1))?;
    function.add_op(Op::LocalStore(2))?;
    function.add_op(Op::Exit)?;
    let (_function_index, machine) = function.finish()?;

    let program_builder = machine.finish()?;
    let descriptor = program_builder.finish_program();

    Ok(descriptor.length)
}
