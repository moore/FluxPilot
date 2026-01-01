#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

#![cfg_attr(
    not(test),
    deny(
        clippy::panic,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::todo,
        clippy::unimplemented,
        clippy::indexing_slicing,
        clippy::string_slice,
        clippy::arithmetic_side_effects,
        clippy::panicking_unwrap,
        clippy::out_of_bounds_indexing,
        clippy::panic_in_result_fn,
        clippy::unwrap_in_result,
    )
)]
#![cfg_attr(not(test), warn(clippy::missing_panics_doc))]

use hal::peripherals;
use hal::prelude::*;
use hal::spi::Spi;

use embassy_executor::Spawner;
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
use pliot::{Pliot, protocol::{ErrorType, Protocol}, meme_storage::MemStorage};
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
const NUM_LEDS: usize = 25;

type ProtocolType = Protocol<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE>;

static CHANNEL: StaticCell<Channel<CriticalSectionRawMutex, Vec<u8, INCOMING_MESSAGE_CAP>, 1>> = StaticCell::new();
static OUTGOING_CHANNEL: StaticCell<Channel<
    CriticalSectionRawMutex,
    Vec<u8, OUTGOING_MESSAGE_CAP>,
    OUTGOING_QUEUE_DEPTH,
>> = StaticCell::new();

static PROGRAM_BUFFER: StaticCell<[u16; 100]> = StaticCell::new();
static GLOBALS: StaticCell<[u16; 10]> = StaticCell::new();

static USB_RECEIVE_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static VM_STACK: StaticCell<Vec<Word, 100>> = StaticCell::new();
static RAW_MESSAGE_BUFF: StaticCell<Vec<u8, INCOMING_MESSAGE_CAP>> = StaticCell::new();
static USB_CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
static USB_BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
static USB_CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static USB_STATE: StaticCell<State> = StaticCell::new();
static LED_BUFFER: StaticCell<[RGB8; NUM_LEDS]> = StaticCell::new();

#[embassy_executor::main(entry = "qingke_rt::entry")]
async fn main(spawner: Spawner) {
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
    let config_descriptor = USB_CONFIG_DESCRIPTOR.init([0; 256]);
    let bos_descriptor = USB_BOS_DESCRIPTOR.init([0; 256]);
    let control_buf = USB_CONTROL_BUF.init([0; 64]);
    let state = USB_STATE.init(State::new());

    let mut builder = Builder::new(
        driver,
        config,
        config_descriptor,
        bos_descriptor,
        &mut [], // no msos descriptors
        control_buf,
    );

    // Create classes on the builder.
    let class = CdcAcmClass::new(&mut builder, state, 64);

    // Build the builder.
    let usb = builder.build();

    /////////////////////////////////////////////////
    let program_buffer = PROGRAM_BUFFER.init([0u16; 100]);
    let globals = GLOBALS.init([0u16; 10]);
    let channel = CHANNEL.init(Channel::new());
    let sender = channel.sender();
    let outgoing_channel = OUTGOING_CHANNEL.init(Channel::new());
    let outgoing_sender = outgoing_channel.sender();
    let outgoing_receiver = outgoing_channel.receiver();

    let (usb_sender, usb_receiver) = class.split();

    ////////////////////////////////////////////

    // SPI1
    let mosi = p.PA7;

    let mut spi_config = hal::spi::Config::default();
    spi_config.frequency = Hertz::mhz(3);
    spi_config.mode = ws2812::MODE;

    let spi = Spi::new_blocking_txonly_nosck(p.SPI1, mosi, spi_config);

    let mut ws = Ws2812::new(spi);

    let data = LED_BUFFER.init([RGB8::default(); NUM_LEDS]);

    // BUG: we should check the values and log + restart
    // If we cant't start these the device will not work.
    let _ = spawner.spawn(usb_device_task(usb));
    let _ = spawner.spawn(usb_rx_task(usb_receiver, sender));
    let _ = spawner.spawn(usb_tx_task(usb_sender, outgoing_receiver));
    led_task(
            &mut ws,
            data,
            channel,
            outgoing_sender,
            program_buffer,
            globals,
        ).await;
}

#[embassy_executor::task]
async fn usb_device_task(
    mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USBD>>,
) {
    usb.run().await;
}

#[embassy_executor::task]
async fn usb_rx_task(
    mut receiver: UsbReceiver<'static, Driver<'static, peripherals::USBD>>,
    incoming: Sender<'static, CriticalSectionRawMutex, Vec<u8, INCOMING_MESSAGE_CAP>, 1>,
) {
    loop {
        receiver.wait_connection().await;
        let _ = usb_receiver_loop(&mut receiver, &incoming).await;
    }
}

#[embassy_executor::task]
async fn usb_tx_task(
    mut sender: UsbSender<'static, Driver<'static, peripherals::USBD>>,
    outgoing: Receiver<
        'static,
        CriticalSectionRawMutex,
        Vec<u8, OUTGOING_MESSAGE_CAP>,
        OUTGOING_QUEUE_DEPTH,
    >,
) {
    loop {
        sender.wait_connection().await;
        let _ = usb_sender_loop(&mut sender, &outgoing).await;
    }
}

async fn led_task(
    ws: &mut Ws2812<Spi<'static, peripherals::SPI1, hal::mode::Blocking>>,
    data: &mut [RGB8; NUM_LEDS],
    channel: &'static Channel<CriticalSectionRawMutex, Vec<u8, INCOMING_MESSAGE_CAP>, 1>,
    outgoing_sender: Sender<'static, CriticalSectionRawMutex, Vec<u8, OUTGOING_MESSAGE_CAP>, OUTGOING_QUEUE_DEPTH>,
    program_buffer: &'static mut [u16; 100],
    globals: &'static mut [u16; 10],
) {
    if get_program(program_buffer).is_err() {
        // BUG: we should log here.
        return;
    }
    let mut storage = MemStorage::new(program_buffer.as_mut_slice());

    let memory = globals.as_mut_slice();
    let mut pliot =
        Pliot::<MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, MemStorage>::new(&mut storage, memory);

    let stack = VM_STACK.init(Vec::new());
    loop {
        
        for (i, led) in data.iter_mut().enumerate() {
            if let Ok((red, green, blue)) = pliot.get_led_color(0, i as u16, stack) {
                *led = (red, green, blue).into();
                stack.clear();
            }
        }
        // BUG: we need to log and error or update an error
        // counter instead of silighently ignoring the error
        let _ = ws.write(brightness(data.iter().cloned(), 2));

        if let Ok(mut message) = channel.try_receive() {
            let mut out_buf = [0u8; OUTGOING_MESSAGE_CAP];
            let wrote = match pliot.process_message(
                stack,
                message.as_mut_slice(),
                out_buf.as_mut_slice(),
            ) {
                Ok(wrote) => wrote,
                Err(_) => {
                    // BUG: we should log error
                    stack.clear();
                    0
                }
            };

            if wrote > 0 {
                let mut response: Vec<u8, OUTGOING_MESSAGE_CAP> = Vec::new();
                if let Some(bytes) = out_buf.get(..wrote) {
                    
                    if response.extend_from_slice(bytes).is_ok() {
                        let _ = outgoing_sender.try_send(response);
                    }
                } else {
                    //BUG: this should be unreachable but we should log
                    // to catch bugs introduced in refactoring.
                }
            }

        }
        Timer::after_millis(10).await;
    }
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

    let mut function = machine.new_function()?;
    function.add_op(Op::Load(0))?;
    function.add_op(Op::Load(1))?;
    function.add_op(Op::Load(2))?;
    function.add_op(Op::Return)?;
    let (_function_index, machine) = function.finish()?;

    let _program_builder = machine.finish()?;

    Ok(())
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => Disconnected {},
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
        let Some(data) = buf.get(..n) else {
            // This should be unreachable but we should probbly
            // log something.
            continue;  
        };
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
