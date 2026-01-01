#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use hal::peripherals;
use hal::prelude::*;
use hal::spi::Spi;

use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver as UsbReceiver, Sender as UsbSender, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::Builder;

use ch32_hal::bind_interrupts;
use ch32_hal::usbd::Driver;
use ch32_hal::usbd::Instance;

use {ch32_hal as hal, panic_halt as _};

use crate::ws2812::Ws2812;
use smart_leds::{brightness, SmartLedsWrite, RGB8};
use ws2812_spi as ws2812;

use light_machine::{
    builder::{MachineBuilderError, Op, ProgramBuilder},
    Program, Word,
};
use pliot::{Pliot, protocol::Protocol, meme_storage::MemStorage};
use postcard::from_bytes_cobs;

use heapless::Vec;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USB_LP_CAN1_RX0 => ch32_hal::usbd::InterruptHandler<peripherals::USBD>;
});

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 100;
const INCOMING_MESSAGE_CAP: usize = 128;
const OUTGOING_MESSAGE_CAP: usize = 128;
const OUTGOING_QUEUE_DEPTH: usize = 1;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>;

static CHANNEL: StaticCell<Channel<CriticalSectionRawMutex, Vec<u8, INCOMING_MESSAGE_CAP>, 1>> = StaticCell::new();
static OUTGOING_CHANNEL: StaticCell<Channel<
    CriticalSectionRawMutex,
    Vec<u8, OUTGOING_MESSAGE_CAP>,
    OUTGOING_QUEUE_DEPTH,
>> = StaticCell::new();

static USB_RECEIVE_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static VM_STACK: StaticCell<Vec<Word, 100>> = StaticCell::new();
static RAW_MESSAGE_BUFF: StaticCell<Vec<u8, INCOMING_MESSAGE_CAP>> = StaticCell::new();

#[embassy_executor::main(entry = "qingke_rt::entry")]
async fn main(_spawner: Spawner) {
    hal::debug::SDIPrint::enable();
    let mut config = hal::Config::default();
    config.rcc = hal::rcc::Config::SYSCLK_FREQ_144MHZ_HSI;
    let p = hal::init(config);

    let usb = p.USBD;

    /* USB DRIVER SECION */
    let driver = Driver::new(usb, Irqs, p.PA12, p.PA11);

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0x4348, 0x55e0);
    //let mut config = embassy_usb::Config::new(0x303A, 0x3001);

    //let mut config = embassy_usb::Config::new(0xC0DE, 0xCAFE);

    config.manufacturer = Some("0xa9f4");
    config.product = Some("FluxPilot");
    config.serial_number = Some("314159");

    // Windows compatibility requires these; CDC-ACM
    //config.device_class = 0x02;
    // But we sue 0xff becouse we need it to not be bound
    // by Android to work on mobil chome.
    config.device_class = 0xFF;
    config.device_sub_class = 0x00;
    config.device_protocol = 0x00;
    config.composite_with_iads = false;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 64];

    let mut state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [], // no msos descriptors
        &mut control_buf,
    );

    // Create classes on the builder.
    let class = CdcAcmClass::new(&mut builder, &mut state, 64);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    /////////////////////////////////////////////////

    let mut program_buffer = [0u16; 100];
    get_program(&mut program_buffer).expect("could not get program");
    let mut globals = [0u16; 10];
    let stack = VM_STACK.init(Vec::new());
    let channel = CHANNEL.init(Channel::new());
    let sender = channel.sender();
    let outgoing_channel = OUTGOING_CHANNEL.init(Channel::new());
    let outgoing_sender = outgoing_channel.sender();
    let outgoing_receiver = outgoing_channel.receiver();

    let (mut usb_sender, mut usb_receiver) = class.split();

    let usb_rx_fut = async {
        loop {
            usb_receiver.wait_connection().await;
            let _ = usb_receiver_loop(&mut usb_receiver, &sender).await;
        }
    };

    let usb_tx_fut = async {
        loop {
            usb_sender.wait_connection().await;
            let _ = usb_sender_loop(&mut usb_sender, &outgoing_receiver).await;
        }
    };

    ////////////////////////////////////////////

    // SPI1
    let mosi = p.PA7;

    let mut spi_config = hal::spi::Config::default();
    spi_config.frequency = Hertz::mhz(3);
    spi_config.mode = ws2812::MODE;

    let spi = Spi::new_blocking_txonly_nosck(p.SPI1, mosi, spi_config);

    let mut ws = Ws2812::new(spi);

    const NUM_LEDS: usize = 25;
    let mut data = [RGB8::default(); NUM_LEDS];

    let mut storage = MemStorage::new(program_buffer.as_mut_slice());


    let memory = globals.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, MemStorage>::new(&mut storage, memory);

    let led_loop = async {
        let mut use_program = false;
        loop {
            for j in 0..(256 * 5) {
                while let Ok(mut message) = channel.try_receive() {
                    let mut out_buf = [0u8; OUTGOING_MESSAGE_CAP];
                    let wrote = pliot
                        .process_message(stack, message.as_mut_slice(), out_buf.as_mut_slice())
                        .expect("Call had error");

                    if wrote > 0 {
                        let mut response: Vec<u8, OUTGOING_MESSAGE_CAP> = Vec::new();
                        if response.extend_from_slice(&out_buf[..wrote]).is_ok() {
                            let _ = outgoing_sender.try_send(response);
                        }
                    }

                    use_program = true;
                }
                for i in 0..NUM_LEDS {
                    if use_program {
                        if let Ok((red, green, blue)) =
                            pliot.get_led_color(0, i as u16, stack)
                        {
                            data[i] = (red, green, blue).into();
                            stack.clear();
                        }
                    } else {
                        data[i] = wheel(
                            (((i * 256) as u16 / NUM_LEDS as u16 + j as u16) & 255) as u8,
                        );
                    }
                }
                ws.write(brightness(data.iter().cloned(), 2)).unwrap();
                Timer::after_millis(10).await;
            }
        }
    };

    // Run everything concurrently.
    // If we had made everything `'static` above instead, we could do this using separate tasks instead.
    join4(usb_fut, usb_rx_fut, usb_tx_fut, led_loop).await;
}

fn get_program(buffer: &mut [u16]) -> Result<(), MachineBuilderError> {
    const MACHINE_COUNT: usize = 1;
    const FUNCTION_COUNT: usize = 2;
    let program_builder =
        ProgramBuilder::<'_, MACHINE_COUNT, FUNCTION_COUNT>::new(buffer, MACHINE_COUNT as u16)?;

    let globals_size = 3;
    let machine = program_builder.new_machine(FUNCTION_COUNT as u16, globals_size)?;
    let mut function = machine.new_function()?;
    function.add_op(Op::Store(0))?;
    function.add_op(Op::Store(1))?;
    function.add_op(Op::Store(2))?;
    function.add_op(Op::Return)?;
    let (_function_index, machine) = function.finish()?;

    let mut function = machine
        .new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Load(0))?;
    function.add_op(Op::Load(1))?;
    function.add_op(Op::Load(2))?;
    function.add_op(Op::Return)?;
    let (_function_index, machine) = function.finish()?;

    let _program_builder = machine.finish()?;

    Ok(())
}

/// Input a value 0 to 255 to get a color value
/// The colours are a transition r - g - b - back to r.
fn wheel(mut wheel_pos: u8) -> RGB8 {
    wheel_pos = 255 - wheel_pos;
    if wheel_pos < 85 {
        return (255 - wheel_pos * 3, 0, wheel_pos * 3).into();
    }
    if wheel_pos < 170 {
        wheel_pos -= 85;
        return (0, wheel_pos * 3, 255 - wheel_pos * 3).into();
    }
    wheel_pos -= 170;
    (wheel_pos * 3, 255 - wheel_pos * 3, 0).into()
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

async fn usb_receiver_loop<'d, T: Instance + 'd>(
    receiver: &mut UsbReceiver<'d, Driver<'d, T>>,
    incoming: &Sender<'static, CriticalSectionRawMutex, Vec<u8, INCOMING_MESSAGE_CAP>, 1>,
) -> Result<(), Disconnected> {
    let buf = USB_RECEIVE_BUF.init([0u8; 64]);
    let frame = RAW_MESSAGE_BUFF.init(Vec::new());
    loop {
        let n = receiver.read_packet(buf).await?;
        let data = &buf[..n];
        for &byte in data {
            if frame.push(byte).is_err() {
                // TODO: Track overflow and discard bytes until the next 0 delimiter to avoid decoding a partial frame.
                // TODO: Send an error response so the sender knows the frame exceeded the size limit.
                frame.clear();
                continue;
            }

            if byte == 0 {
                let message = frame.clone();
                frame.clear();
                let _ = incoming.try_send(message);
            }
        }
    }
}

async fn usb_sender_loop<'d, T: Instance + 'd>(
    sender: &mut UsbSender<'d, Driver<'d, T>>,
    outgoing: &Receiver<
        'static,
        CriticalSectionRawMutex,
        Vec<u8, OUTGOING_MESSAGE_CAP>,
        OUTGOING_QUEUE_DEPTH,
    >,
) -> Result<(), Disconnected> {
    loop {
        let message = outgoing.receive().await;
        sender.write_packet(message.as_slice()).await?;
    }
}

fn handle_message<const STACK_SIZE: usize>(
    message: &mut [u8],
    program: &mut Program<'_, '_>,
    stack: &mut Vec<Word, STACK_SIZE>,
) -> bool {
    let Ok(decoded) = from_bytes_cobs::<ProtocolType>(message) else {
        // TODO: Send an error response so the sender knows the message was too large or corrupted.
        stack.clear();
        return false;
    };

    match decoded {
        Protocol::Call { function, args, .. } => {
            let Ok(function_index) = function.function_index.try_into() else {
                stack.clear();
                return false;
            };

            for arg in args {
                if stack.push(arg).is_err() {
                    stack.clear();
                    return false;
                }
            }

            let result = program.call(function.machine_index, function_index, stack);
            stack.clear();
            result.is_ok()
        }
        _ => false,
    }
}
