use super::*;

struct MachineData {
    globals: [Word; 10],
    static_data: [Word; 0],
    program: [Word; 100],
    main: usize,
    init: usize,
}

fn get_test_program() -> MachineData {
    let globals = [0u16; 10];
    let static_data = [0u16; 0];
    let mut program = [0u16; 100];

    // main
    // globals, 0, 1, 2, on to stack
    program[0] = Ops::Load.into();
    program[1] = 0;
    program[2] = Ops::Load.into();
    program[3] = 1;
    program[4] = Ops::Load.into();
    program[5] = 2;
    program[6] = Ops::Return.into();

    // init stor the top three entires in to globals 0, 1, 2
    program[7] = Ops::Store.into();
    program[8] = 0;
    program[9] = Ops::Store.into();
    program[10] = 1;
    program[11] = Ops::Store.into();
    program[12] = 2;
    program[13] = Ops::Return.into();

    MachineData {
        globals,
        static_data,
        program,
        main: 0,
        init: 7,
    }
}

#[test]
fn test_init_get_color() -> Result<(), MachineError> {
    let mut md = get_test_program();

    let mut machine = Machine::new(
        &md.static_data,
        &mut md.globals,
        &md.program,
        md.init,
        md.main,
    )?;

    let (red, green, blue) = (17, 23, 31);
    let mut stack: Vec<Word, 100> = Vec::new();
    stack.push(red).unwrap();
    stack.push(green).unwrap();
    stack.push(blue).unwrap();
    machine.init(&mut stack)?;

    assert_eq!(stack.len(), 0);

    let (r, g, b) = machine
        .get_led_color(31337, &mut stack)
        .expect("Could not get led color");

    assert_eq!(stack.len(), 1); // 1 becouse we leave the led index on the stack in our test
    assert_eq!(stack[0], 31337);
    assert_eq!((r as u16, g as u16, b as u16), (red, green, blue));
    Ok(())
}
