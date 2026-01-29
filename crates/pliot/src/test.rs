use crate::{
    meme_storage::MemStorage,
    protocol::{Controler, FunctionId},
};

use super::*;
use light_machine::assembler::Assembler;
use light_machine::builder::*;
use postcard::{from_bytes_cobs, to_vec_cobs};

extern crate std;
use std::format;
use std::println;
use std::string::{String, ToString};
use std::vec::Vec as StdVec;
use std::vec;

use heapless::Vec;

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 100;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>;

fn build_simple_crawler_machine_lines(name: &str, init: [Word; 6]) -> StdVec<String> {
    let source = format!(
        "
.machine {} locals 6 functions 7
    .local red 0
    .local green 1
    .local blue 2
    .local speed 3
    .local brightness 4
    .local led_count 5
    .data control_statics
    init_red:
    .word {}
    init_green:
    .word {}
    init_blue:
    .word {}
    init_speed:
    .word {}
    init_brightness:
    .word {}
    init_led_count:
    .word {}
    .end

    .func init index 0
        LOAD_STATIC init_red
        STORE red
        LOAD_STATIC init_green
        STORE green
        LOAD_STATIC init_blue
        STORE blue
        LOAD_STATIC init_speed
        STORE speed
        LOAD_STATIC init_brightness
        STORE brightness
        LOAD_STATIC init_led_count
        STORE led_count
        EXIT
    .end

    .func set_rgb index 2
        STORE blue
        STORE green
        STORE red
        EXIT
    .end

    .func set_brightness index 3
        STORE brightness
        EXIT
    .end

    .func set_speed index 4
        STORE speed
        EXIT
    .end

    .func set_led_count index 6
        STORE led_count
        EXIT
    .end

    .func get_rgb_worker index 5
        .frame sred 0
        .frame sgreen 1
        .frame sblue 2
        .frame led_index 3
        .frame ticks 4
        SLOAD led_index
        SLOAD ticks
        LOAD speed
        LOAD led_count
        MUL
        MOD
        LOAD speed
        DIV
        BREQ match
        SLOAD sred
        SLOAD sgreen
        SLOAD sblue
        RET 3
        match:
        LOAD red
        LOAD brightness
        MUL
        PUSH 100
        DIV
        LOAD green
        LOAD brightness
        MUL
        PUSH 100
        DIV
        LOAD blue
        LOAD brightness
        MUL
        PUSH 100
        DIV
        RET 3
    .end

    .func get_rgb index 1
        PUSH 5
        CALL get_rgb_worker
        EXIT
    .end
.end",
        name, init[0], init[1], init[2], init[3], init[4], init[5]
    );

    source
        .lines()
        .map(|line| line.to_string())
        .collect::<StdVec<String>>()
}

#[test]
fn test_pilot_get_color() -> Result<(), MachineError> {
    let mut buffer = [0u16; 30];
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 3;
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
    function.add_op(Op::Exit).expect("could not add op");
    let (_, machine) = function.finish().expect("Could not finish function");

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Load(0)).expect("could not add op");
    function.add_op(Op::Load(1)).expect("could not add op");
    function.add_op(Op::Load(2)).expect("could not add op");
    function.add_op(Op::Exit).expect("could not add op");
    let (_, machine) = function.finish().expect("Could not finish function");

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Store(0)).expect("could not add op");
    function.add_op(Op::Store(1)).expect("could not add op");
    function.add_op(Op::Store(2)).expect("could not add op");
    function.add_op(Op::Exit).expect("could not add op");
    let (_, machine) = function.finish().expect("Could not finish function");

    let program_builder = machine.finish().expect("Could not finish program");

    let descriptor = program_builder.finish_program();

    let program = &buffer[0..descriptor.length];

    println!("program {:?}", program);

    let mut storage_buffer = [0u16; 100];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice());

    let mut controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE> = Controler::new();

    let mut stack: Vec<Word, 100> = Vec::new();
    let mut globals = [0u16; 10];
    let memory = globals.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, MemStorage>::new(&mut storage, memory);

    let loader = controler.get_program_loader(program);

    let mut out_buf = vec![0u8; 1024];

    for message in loader {
        let mut in_buf = to_vec_cobs::<ProtocolType, 2048>(&message).unwrap();

        let wrote = pliot
            .process_message(&mut stack, &mut in_buf[..], out_buf.as_mut_slice())
            .expect("Call had error");

        assert_eq!(0, wrote);
    }

    let (red, green, blue) = (17, 23, 31);

    {
        let mut args = Vec::<u16, MAX_ARGS>::new();

        args.push(red).unwrap();
        args.push(green).unwrap();
        args.push(blue).unwrap();

        let store_function_index = descriptor.machines[0].functions[2].clone();
        let function = FunctionId {
            machine_index: 0,
            function_index: store_function_index.into(),
        };

        let message = controler.call(function, args);

        let mut in_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();

        let wrote = pliot
            .process_message(&mut stack, &mut in_buf[..], out_buf.as_mut_slice())
            .expect("Call had error");

        assert_ne!(0, wrote);

        let response: ProtocolType =
            from_bytes_cobs(&mut out_buf[..wrote]).expect("could not read response");

        println!("return was {:?}", &response);

        match &response {
            Protocol::Return {
                request_id: _,
                result,
            } => {
                assert_eq!(result.len(), 0);
            }
            _ => panic!("response was not Return"),
        }
        assert_eq!(message.get_request_id(), response.get_request_id());
    }

    assert_eq!(stack.len(), 0);

    {
        let args = Vec::<u16, MAX_ARGS>::new();

        println!("stack is {:?}", stack);
        let get_function_index = descriptor.machines[0].functions[1].clone();
        let function = FunctionId {
            machine_index: 0,
            function_index: get_function_index.into(),
        };

        println!("function id {:?}", &function);

        let message = controler.call(function, args);

        let mut in_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();

        let wrote = pliot
            .process_message(&mut stack, &mut in_buf[..], out_buf.as_mut_slice())
            .expect("Call had error");

        assert_ne!(0, wrote);

        let response: ProtocolType =
            from_bytes_cobs(&mut out_buf[..wrote]).expect("could not read response");

        println!("return was {:?}", &response);

        match &response {
            Protocol::Return {
                request_id: _,
                result,
            } => {
                assert_eq!(result.len(), 3);
                let r = stack.pop().unwrap();
                let g = stack.pop().unwrap();
                let b = stack.pop().unwrap();

                assert_eq!((r as u16, g as u16, b as u16), (red, green, blue));
            }
            _ => panic!("response was not Return"),
        }
    }

    Ok(())
}

#[test]
fn test_pilot_four_simple_crawlers_in_one_program() -> Result<(), PliotError> {
    const MACHINE_COUNT: usize = 4;
    const FUNCTION_COUNT: usize = 7;
    const LABEL_CAP: usize = 32;
    const DATA_CAP: usize = 32;
    const STACK_CAP: usize = 32;

    let mut buffer = [0u16; 512];
    let builder =
        ProgramBuilder::<MACHINE_COUNT, FUNCTION_COUNT>::new(&mut buffer, MACHINE_COUNT as Word)
            .unwrap();
    let mut asm: Assembler<MACHINE_COUNT, FUNCTION_COUNT, LABEL_CAP, DATA_CAP> =
        Assembler::new(builder);

    let init_values: [[Word; 6]; MACHINE_COUNT] = [
        [10, 20, 30, 2, 100, 256],
        [40, 50, 60, 3, 80, 256],
        [70, 80, 90, 4, 60, 256],
        [15, 25, 35, 5, 90, 256],
    ];

    for (index, init) in init_values.iter().enumerate() {
        let name = format!("crawler{}", index + 1);
        let lines = build_simple_crawler_machine_lines(&name, *init);
        for line in lines.iter() {
            asm.add_line(line).unwrap();
        }
    }

    let descriptor = asm.finish().unwrap();
    let program = &buffer[..descriptor.length];

    let mut storage_buffer = [0u16; 2048];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice());
    let mut controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE> = Controler::new();

    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    let mut globals = [0u16; 32];
    let memory = globals.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, MemStorage>::new(&mut storage, memory);

    let loader = controler.get_program_loader(program);
    let mut out_buf = vec![0u8; 1024];

    for message in loader {
        let mut in_buf = to_vec_cobs::<ProtocolType, 2048>(&message).unwrap();
        let wrote = pliot.process_message(&mut stack, &mut in_buf[..], out_buf.as_mut_slice())?;
        assert_eq!(0, wrote);
    }

    let machine_count = pliot.machine_count()?;
    assert_eq!(machine_count, MACHINE_COUNT as Word);

    for (machine_index, init) in init_values.iter().enumerate() {
        stack.clear();
        stack.push(0).unwrap();
        stack.push(0).unwrap();
        stack.push(0).unwrap();
        let (r, g, b) =
            pliot.get_led_color(machine_index as Word, 0, 0, &mut stack)?;
        let expected_r = (init[0] * init[4]) / 100;
        let expected_g = (init[1] * init[4]) / 100;
        let expected_b = (init[2] * init[4]) / 100;
        assert_eq!(
            (r, g, b),
            (expected_r as u8, expected_g as u8, expected_b as u8)
        );
    }

    for i in 8000..8100 {
        for j in 0..256 {
            for machine_index in 0..machine_count {
                stack.clear();
                stack.push(0).unwrap();
                stack.push(0).unwrap();
                stack.push(0).unwrap();
                pliot.get_led_color(machine_index, j, i, &mut stack)?;
            }
        }
    }

    Ok(())
}
