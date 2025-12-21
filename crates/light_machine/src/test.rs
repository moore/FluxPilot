use super::*;
use builder::*;

extern crate std;
use std::println;

#[test]
fn test_init_get_color() -> Result<(), MachineError> {
    let mut buffer = [0u16; 30];
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 2;
    let program_builder =
        ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(&mut buffer, MACHINE_COUNT as u16)
            .expect("could not get machine builder");

    let globals_size = 3;
    let machine = program_builder
        .new_machine(FUNCTION_COUNT as u16, globals_size)
        .expect("could not get new machine");

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Store(0)).expect("could not add op");
    function.add_op(Op::Store(1)).expect("could not add op");
    function.add_op(Op::Store(2)).expect("could not add op");
    function.add_op(Op::Return).expect("could not add op");
    let (_function_index, machine) = function.finish().expect("could not finish function");

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Load(0)).expect("could not add op");
    function.add_op(Op::Load(1)).expect("could not add op");
    function.add_op(Op::Load(2)).expect("could not add op");
    function.add_op(Op::Return).expect("could not add op");
    let (_function_index, machine) = function.finish().expect("Could not finish function");

    let _program_builder = machine.finish();

    println!("program {:?}", buffer);

    let mut globals = [0u16; 10];
    let (red, green, blue) = (17, 23, 31);
    let mut stack: Vec<Word, 100> = Vec::new();

    {
        let mut program = Program::new(buffer.as_slice(), globals.as_mut_slice())?;

        stack.push(red).unwrap();
        stack.push(green).unwrap();
        stack.push(blue).unwrap();
        program.init_machine(0, &mut stack)?;
    }
    assert_eq!(stack.len(), 0);

    println!("memory {:?}", globals);

    {
        let mut program = Program::new(buffer.as_slice(), globals.as_mut_slice())?;

        println!("stack is {:?}", stack);

        let (r, g, b) = program
            .get_led_color(0, 31337, &mut stack)
            .expect("Could not get led color");

        println!("stack is {:?}", stack);
        assert_eq!((r as u16, g as u16, b as u16), (red, green, blue));
    }

    assert_eq!(stack.len(), 1); // 1 becouse we leave the led index on the stack in our test
    assert_eq!(stack[0], 31337);
    Ok(())
}
