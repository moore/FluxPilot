use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_usb::driver::{Driver, EndpointError};
use heapless::Vec;
use light_machine::Word;
use pliot::Pliot;

use crate::usb_vendor::{VendorReceiver, VendorSender};

pub struct PliotShared<
    'a,
    'b,
    S: pliot::Storage,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
    const STACK_SIZE: usize,
> {
    pub pliot: Pliot<'a, 'b, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, S>,
    pub stack: Vec<Word, STACK_SIZE>,
}

pub struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => Disconnected {},
            EndpointError::Disabled => Disconnected {},
        }
    }
}

pub async fn io_loop<
    'd,
    'a,
    'b,
    D,
    S,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
    const STACK_SIZE: usize,
    const USB_BUF_SIZE: usize,
    const IN_CAP: usize,
    const OUT_CAP: usize,
>(
    receiver: &mut VendorReceiver<'d, D>,
    sender: &mut VendorSender<'d, D>,
    shared: &'static Mutex<
        CriticalSectionRawMutex,
        PliotShared<'a, 'b, S, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE, STACK_SIZE>,
    >,
    usb_buf: &mut [u8; USB_BUF_SIZE],
    frame: &mut Vec<u8, IN_CAP>,
) -> Result<(), Disconnected>
where
    D: Driver<'d>,
    S: pliot::Storage,
{
    loop {
        let n = receiver.read_packet(usb_buf).await?;
        let Some(data) = usb_buf.get(..n) else {
            continue;
        };
        for &byte in data {
            if frame.push(byte).is_err() {
                frame.clear();
                continue;
            }

            if byte == 0 {
                let mut out_buf = [0u8; OUT_CAP];
                let wrote = {
                    let mut guard = shared.lock().await;
                    let PliotShared { pliot, stack } = &mut *guard;
                    let result = pliot.process_message(
                        stack,
                        frame.as_mut_slice(),
                        out_buf.as_mut_slice(),
                    );

                    match result {
                        Ok(wrote) => wrote,
                        Err(_) => {
                            stack.clear();
                            0
                        }
                    }
                };
                frame.clear();

                if wrote > 0 {
                    if let Some(bytes) = out_buf.get(..wrote) {
                        sender.write_packet(bytes).await?;
                    }
                }
            }
        }
    }
}
