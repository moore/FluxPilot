use wasm_bindgen::prelude::*;

use pliot::protocol::{Controler, FunctionId, Protocol, RequestId};

use light_machine::{ProgramDescriptor, Word, builder::*};
use postcard::{to_vec_cobs};

use heapless::Vec;

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 100;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>;

#[wasm_bindgen]
pub enum FlightDeckError {
    ToManyArguments
}
#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);
}

#[wasm_bindgen(module = "/deck.js")]
extern "C" {
    pub fn send(bytes: &[u8]);
}


#[wasm_bindgen]
pub struct WasmRequestId(u64);

impl From<RequestId> for WasmRequestId {
    fn from(id: RequestId) -> Self { Self(id.value()) }
}

#[wasm_bindgen]
pub struct FlightDeck {
    controler: Controler<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>,
}



#[wasm_bindgen]
impl FlightDeck {
    #[wasm_bindgen(constructor)]
    pub fn new() -> FlightDeck {
        FlightDeck {
            controler: Controler::new(),
        }
    }

    pub fn call(&mut self, machine_index: Word, function_index: u32, args: &[Word]) -> Result<Option<WasmRequestId>, FlightDeckError> {
        if args.len() > MAX_ARGS {
            return Err(FlightDeckError::ToManyArguments);
        }

        let mut call_args = Vec::<Word, MAX_ARGS>::new();

        for arg in args {
           if call_args.push(*arg).is_err() {
            unreachable!("The check abouve makes this unreachable");
           }
        }

        let function = FunctionId {
            machine_index,
            function_index,
        };

        let message = self.controler.call(function, call_args);

        let message_buf = to_vec_cobs::<ProtocolType, 100>(&message).unwrap();
        let bytes = message_buf.as_slice();
        send(bytes);

        let request_id = message.get_request_id();

        Ok(request_id.map(Into::into))
    }
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
