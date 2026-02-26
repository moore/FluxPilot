use crate::{
    meme_storage::MemStorage,
    protocol::{Controler, ErrorType, FunctionId, MessageType, RequestId},
};

use super::*;
use light_machine::assembler::Assembler;
use light_machine::builder::*;
use light_machine::{ProgramWord, StackWord};
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
const UI_BLOCK_SIZE: usize = 128;
const SHARED_FUNCTION_COUNT: u16 = 4;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>;

fn add_shared_stubs<const MACHINE_COUNT: usize, const FUNCTION_COUNT: usize>(
    mut builder: ProgramBuilder<'_, MACHINE_COUNT, FUNCTION_COUNT>,
) -> ProgramBuilder<'_, MACHINE_COUNT, FUNCTION_COUNT> {
    for index in 0..SHARED_FUNCTION_COUNT {
        let mut shared_function = builder
            .new_shared_function_at_index(FunctionIndex::new(index))
            .expect("could not get shared function builder");
        shared_function
            .add_op(Op::Exit)
            .expect("could not add op");
        let (_index, next_program) = shared_function
            .finish()
            .expect("could not finish shared function");
        builder = next_program;
    }
    builder
}

fn add_shared_stubs_from<const MACHINE_COUNT: usize, const FUNCTION_COUNT: usize>(
    mut builder: ProgramBuilder<'_, MACHINE_COUNT, FUNCTION_COUNT>,
    start: u16,
) -> ProgramBuilder<'_, MACHINE_COUNT, FUNCTION_COUNT> {
    for index in start..SHARED_FUNCTION_COUNT {
        let mut shared_function = builder
            .new_shared_function_at_index(FunctionIndex::new(index))
            .expect("could not get shared function builder");
        shared_function
            .add_op(Op::Exit)
            .expect("could not add op");
        let (_index, next_program) = shared_function
            .finish()
            .expect("could not finish shared function");
        builder = next_program;
    }
    builder
}

fn build_simple_crawler_machine_lines(name: &str, init: [ProgramWord; 6]) -> StdVec<String> {
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
        LSTORE red
        LOAD_STATIC init_green
        LSTORE green
        LOAD_STATIC init_blue
        LSTORE blue
        LOAD_STATIC init_speed
        LSTORE speed
        LOAD_STATIC init_brightness
        LSTORE brightness
        LOAD_STATIC init_led_count
        LSTORE led_count
        EXIT
    .end

    .func set_rgb index 2
        LSTORE blue
        LSTORE green
        LSTORE red
        EXIT
    .end

    .func set_brightness index 3
        LSTORE brightness
        EXIT
    .end

    .func set_speed index 4
        LSTORE speed
        EXIT
    .end

    .func set_led_count index 6
        LSTORE led_count
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
        LLOAD speed
        LLOAD led_count
        MUL
        MOD
        LLOAD speed
        DIV
        BREQ match
        SLOAD sred
        SLOAD sgreen
        SLOAD sblue
        RET 3
        match:
        LLOAD red
        LLOAD brightness
        MUL
        PUSH 100
        DIV
        LLOAD green
        LLOAD brightness
        MUL
        PUSH 100
        DIV
        LLOAD blue
        LLOAD brightness
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
    let mut buffer = [0u16; 128];
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 3;
    let program_builder = ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(
        &mut buffer,
        MACHINE_COUNT as u16,
        MACHINE_COUNT as u16,
        SHARED_FUNCTION_COUNT,
    )
    .expect("could not get machine builder");

    let program_builder = add_shared_stubs(program_builder);

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
    function.add_op(Op::LocalLoad(0)).expect("could not add op");
    function.add_op(Op::LocalLoad(1)).expect("could not add op");
    function.add_op(Op::LocalLoad(2)).expect("could not add op");
    function.add_op(Op::Exit).expect("could not add op");
    let (_, machine) = function.finish().expect("Could not finish function");

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::LocalStore(2)).expect("could not add op");
    function.add_op(Op::LocalStore(1)).expect("could not add op");
    function.add_op(Op::LocalStore(0)).expect("could not add op");
    function.add_op(Op::Exit).expect("could not add op");
    let (_, machine) = function.finish().expect("Could not finish function");

    let program_builder = machine.finish().expect("Could not finish program");

    let descriptor = program_builder.finish_program();

    let program = &buffer[0..descriptor.length];

    println!("program {:?}", program);

    let mut storage_buffer = [0u16; 100];
    let mut ui_state = [0u8; 512];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice(), ui_state.as_mut_slice());

    let mut controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> =
        Controler::new();

    let mut memory = [0u32; 128];
    let memory = memory.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, MemStorage>::new(
            &mut storage,
            memory,
        );

    let ui_state: [u8; 0] = [];
    let loader = controler.get_program_loader(program, &ui_state);

    let mut out_buf = vec![0u8; 1024];

    for message in loader {
        let mut in_buf = to_vec_cobs::<ProtocolType, 2048>(&message).unwrap();

        let wrote = pliot
            .process_message(&mut in_buf[..], out_buf.as_mut_slice())
            .expect("Call had error");

        assert_eq!(0, wrote);
    }

    let (red, green, blue): (u16, u16, u16) = (17, 23, 31);

    {
        let mut args = Vec::<StackWord, MAX_ARGS>::new();

        args.push(StackWord::from(red)).unwrap();
        args.push(StackWord::from(green)).unwrap();
        args.push(StackWord::from(blue)).unwrap();

        let type_id = descriptor.instances[0].type_id as usize;
        let store_function_index = descriptor.types[type_id].functions[2].clone();
        let function = FunctionId {
            machine_index: 0,
            function_index: store_function_index.into(),
        };

        let message = controler.call(function, args);

        let mut in_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();

        let wrote = pliot
            .process_message(&mut in_buf[..], out_buf.as_mut_slice())
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

    {
        let args = Vec::<StackWord, MAX_ARGS>::new();

        let type_id = descriptor.instances[0].type_id as usize;
        let get_function_index = descriptor.types[type_id].functions[1].clone();
        let function = FunctionId {
            machine_index: 0,
            function_index: get_function_index.into(),
        };

        println!("function id {:?}", &function);

        let message = controler.call(function, args);

        let mut in_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();

        let wrote = pliot
            .process_message(&mut in_buf[..], out_buf.as_mut_slice())
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
                assert_eq!(
                    (result[0] as u16, result[1] as u16, result[2] as u16),
                    (red, green, blue)
                );
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

    let mut buffer = [0u16; 512];
    let builder = ProgramBuilder::<MACHINE_COUNT, FUNCTION_COUNT>::new(
        &mut buffer,
        MACHINE_COUNT as ProgramWord,
        MACHINE_COUNT as ProgramWord,
        SHARED_FUNCTION_COUNT,
    )
    .unwrap();
    let builder = add_shared_stubs(builder);
    let mut asm: Assembler<MACHINE_COUNT, FUNCTION_COUNT, LABEL_CAP, DATA_CAP> =
        Assembler::new(builder);

    let init_values: [[ProgramWord; 6]; MACHINE_COUNT] = [
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
    let mut ui_state = [0u8; 512];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice(), ui_state.as_mut_slice());
    let mut controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> =
        Controler::new();

    let mut memory = [0u32; 256];
    let memory = memory.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, MemStorage>::new(
            &mut storage,
            memory,
        );

    let ui_state: [u8; 0] = [];
    let loader = controler.get_program_loader(program, &ui_state);
    let mut out_buf = vec![0u8; 1024];

    for message in loader {
        let mut in_buf = to_vec_cobs::<ProtocolType, 2048>(&message).unwrap();
        let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
        assert_eq!(0, wrote);
    }

    let machine_count = pliot.machine_count()?;
    assert_eq!(machine_count, MACHINE_COUNT as ProgramWord);

    for (machine_index, init) in init_values.iter().enumerate() {
        let (r, g, b) =
            pliot.get_led_color(machine_index as ProgramWord, 0, 0u32, (0, 0, 0))?;
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
                pliot.get_led_color(machine_index, j, i as u32, (0, 0, 0))?;
            }
        }
    }

    Ok(())
}

#[test]
fn test_call_static_function() -> Result<(), PliotError> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 1;

    let mut buffer = [0u16; 256];
    let program_builder = ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(
        &mut buffer,
        MACHINE_COUNT as u16,
        MACHINE_COUNT as u16,
        SHARED_FUNCTION_COUNT,
    )
    .expect("could not get machine builder");

    let mut shared_function = program_builder
        .new_shared_function_at_index(FunctionIndex::new(0))
        .expect("could not get shared function builder");
    shared_function.add_op(Op::Push(11)).expect("could not add op");
    shared_function.add_op(Op::Push(22)).expect("could not add op");
    shared_function.add_op(Op::Exit).expect("could not add op");
    let (_index, program_builder) = shared_function
        .finish()
        .expect("could not finish shared function");
    let program_builder = add_shared_stubs_from(program_builder, 1);

    let machine = program_builder
        .new_machine(FUNCTION_COUNT as u16, 0)
        .expect("could not get new machine");
    let mut function = machine
        .new_function()
        .expect("could not get function builder");
    function.add_op(Op::Exit).expect("could not add op");
    let (_index, machine) = function.finish().expect("could not finish function");
    let program_builder = machine.finish().expect("could not finish machine");

    let descriptor = program_builder.finish_program();
    let program = &buffer[0..descriptor.length];

    let mut storage_buffer = [0u16; 256];
    let mut ui_state = [0u8; 128];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice(), ui_state.as_mut_slice());

    let mut controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> =
        Controler::new();

    let mut memory = [0u32; 256];
    let memory = memory.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, MemStorage>::new(
            &mut storage,
            memory,
        );

    let ui_state: [u8; 0] = [];
    let loader = controler.get_program_loader(program, &ui_state);
    let mut out_buf = vec![0u8; 1024];

    for message in loader {
        let mut in_buf = to_vec_cobs::<ProtocolType, 2048>(&message).unwrap();
        let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
        assert_eq!(0, wrote);
    }

    let args = Vec::<StackWord, MAX_ARGS>::new();
    let message = controler.call_static(0, args);
    let mut in_buf = to_vec_cobs::<ProtocolType, 256>(&message).unwrap();
    let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
    assert_ne!(0, wrote);

    let response: ProtocolType =
        from_bytes_cobs(&mut out_buf[..wrote]).expect("could not read response");
    match response {
        Protocol::StaticFunctionResult {
            request_id,
            function_id,
            result,
            error,
        } => {
            assert_eq!(message.get_request_id(), Some(request_id));
            assert_eq!(function_id, 0);
            assert!(error.is_none());
            assert_eq!(result.len(), 2);
            assert_eq!((result[0] as u16, result[1] as u16), (11, 22));
        }
        _ => panic!("response was not StaticFunctionResult"),
    }

    Ok(())
}

#[test]
fn test_read_ui_state_blocks() -> Result<(), PliotError> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 1;
    const LABEL_CAP: usize = 16;
    const DATA_CAP: usize = 16;

    let mut buffer = [0u16; 256];
    let builder = ProgramBuilder::<MACHINE_COUNT, FUNCTION_COUNT>::new(
        &mut buffer,
        MACHINE_COUNT as ProgramWord,
        MACHINE_COUNT as ProgramWord,
        SHARED_FUNCTION_COUNT,
    )
    .unwrap();
    let builder = add_shared_stubs(builder);
    let mut asm: Assembler<MACHINE_COUNT, FUNCTION_COUNT, LABEL_CAP, DATA_CAP> =
        Assembler::new(builder);

    let lines = [
        ".machine main locals 0 functions 1",
        "    .func init index 0",
        "      EXIT",
        "    .end",
        ".end",
    ];
    for line in lines.iter() {
        asm.add_line(line).unwrap();
    }

    let descriptor = asm.finish().unwrap();
    let program = &buffer[..descriptor.length];

    let mut storage_buffer = [0u16; 256];
    let mut ui_state_mem = [0u8; 256];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice(), ui_state_mem.as_mut_slice());
    let mut controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> =
        Controler::new();

    let mut memory = [0u32; 128];
    let memory = memory.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, MemStorage>::new(
            &mut storage,
            memory,
        );

    let ui_state: [u8; 5] = [1, 2, 3, 4, 5];
    let loader = controler.get_program_loader(program, &ui_state);
    let mut out_buf = vec![0u8; 1024];

    for message in loader {
        let mut in_buf = to_vec_cobs::<ProtocolType, 2048>(&message).unwrap();
        let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
        assert_eq!(0, wrote);
    }

    let read_block = controler.read_ui_state(0);
    let mut in_buf = to_vec_cobs::<ProtocolType, 256>(&read_block).unwrap();
    let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
    let response: ProtocolType =
        from_bytes_cobs(&mut out_buf[..wrote]).expect("could not read response");
    match response {
        Protocol::UiStateBlock {
            total_size,
            block_number,
            block,
            ..
        } => {
            assert_eq!(total_size, ui_state.len() as u32);
            assert_eq!(block_number, 0);
            assert_eq!(block.as_slice(), ui_state.as_slice());
        }
        _ => panic!("response was not UiStateBlock"),
    }

    let read_block = controler.read_ui_state(1);
    let mut in_buf = to_vec_cobs::<ProtocolType, 256>(&read_block).unwrap();
    let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
    let response: ProtocolType =
        from_bytes_cobs(&mut out_buf[..wrote]).expect("could not read response");
    match response {
        Protocol::UiStateBlock { block, .. } => {
            assert!(block.is_empty());
        }
        _ => panic!("response was not UiStateBlock"),
    }

    Ok(())
}

#[test]
fn test_get_i2c_devices_message() -> Result<(), PliotError> {
    let mut storage_buffer = [0u16; 128];
    let mut ui_state = [0u8; 128];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice(), ui_state.as_mut_slice());
    let mut controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE> =
        Controler::new();
    let mut memory = [0u32; 128];
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, MemStorage>::new(
            &mut storage,
            memory.as_mut_slice(),
        );
    pliot.set_i2c_devices(&[0x10, 0x22, 0x30]);

    let message = controler.get_i2c_devices(1);
    let mut in_buf = to_vec_cobs::<ProtocolType, 256>(&message).unwrap();
    let mut out_buf = [0u8; 256];
    let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
    assert_ne!(0, wrote);

    let response: ProtocolType = from_bytes_cobs(&mut out_buf[..wrote]).unwrap();
    match response {
        Protocol::I2cDevices {
            request_id,
            total_count,
            devices,
        } => {
            assert_eq!(message.get_request_id(), Some(request_id));
            assert_eq!(total_count, 3);
            assert_eq!(devices.as_slice(), &[0x22, 0x30]);
        }
        _ => panic!("response was not I2cDevices"),
    }

    Ok(())
}

#[test]
fn test_i2c_devices_message_is_unexpected_inbound() -> Result<(), PliotError> {
    let mut storage_buffer = [0u16; 128];
    let mut ui_state = [0u8; 128];
    let mut storage = MemStorage::new(storage_buffer.as_mut_slice(), ui_state.as_mut_slice());
    let mut memory = [0u32; 128];
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, MemStorage>::new(
            &mut storage,
            memory.as_mut_slice(),
        );

    let mut devices: Vec<u8, MAX_RESULT> = Vec::new();
    devices.push(0x10).unwrap();
    let mut in_buf = to_vec_cobs::<ProtocolType, 256>(&Protocol::I2cDevices {
        request_id: RequestId::new(7),
        total_count: 1,
        devices,
    })
    .unwrap();
    let mut out_buf = [0u8; 256];
    let wrote = pliot.process_message(&mut in_buf[..], out_buf.as_mut_slice())?;
    assert_ne!(0, wrote);

    let response: ProtocolType = from_bytes_cobs(&mut out_buf[..wrote]).unwrap();
    match response {
        Protocol::Error {
            request_id,
            error_type,
            ..
        } => {
            assert_eq!(request_id, Some(RequestId::new(7)));
            match error_type {
                ErrorType::UnexpectedMessageType(message_type) => {
                    assert!(matches!(message_type, MessageType::I2cDevices));
                }
                _ => panic!("unexpected error type"),
            }
        }
        _ => panic!("response was not Error"),
    }

    Ok(())
}
