use crate::{
    meme_storage::MemStorage,
    protocol::{Controler, FunctionId},
};

use super::*;
use light_machine::builder::*;
use postcard::{from_bytes_cobs, to_vec_cobs};

extern crate std;
use std::println;

use heapless::Vec;

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 100;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>;

#[test]
fn test_pilot_get_color() -> Result<(), MachineError> {
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
    let (_, machine) = function.finish().expect("Could not finish function");

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Load(0)).expect("could not add op");
    function.add_op(Op::Load(1)).expect("could not add op");
    function.add_op(Op::Load(2)).expect("could not add op");
    function.add_op(Op::Return).expect("could not add op");
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
        let mut in_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();

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

        let store_function_index = descriptor.machines[0].functions[0].clone();
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
