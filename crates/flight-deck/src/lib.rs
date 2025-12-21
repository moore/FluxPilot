use wasm_bindgen::prelude::*;

use pliot::protocol::{Controler, FunctionId, Protocol};

use light_machine::{ProgramDescriptor, builder::*};
use postcard::{from_bytes_cobs, to_vec_cobs};

use heapless::Vec;

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 100;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>;
#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);
}

#[wasm_bindgen]
pub fn greet(name: &str) {
    alert(&format!("Hello, {}!", name));
}

fn get_test_program(buffer: &mut [u16]) -> ProgramDescriptor<1, 2> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 2;
    let program_builder =
        ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(buffer, MACHINE_COUNT as u16)
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
    let (_store_function_index, machine) = function.finish().expect("Could not finishe function");

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Load(0)).expect("could not add op");
    function.add_op(Op::Load(1)).expect("could not add op");
    function.add_op(Op::Load(2)).expect("could not add op");
    function.add_op(Op::Return).expect("could not add op");
    let (_get_function_index, machine) = function.finish().expect("Could not finish function");

    let program_builder = machine.finish().expect("Could not finish machine");

    let descriptor = program_builder.finish_program();

    descriptor
}
