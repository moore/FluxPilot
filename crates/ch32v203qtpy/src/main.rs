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
use embassy_sync::mutex::Mutex;
use embassy_time::{Timer, Instant, Duration};
use embassy_usb::driver::EndpointError;
use embassy_usb::Builder;

use ch32_hal::bind_interrupts;
use ch32_hal::usbd::Driver;
use ch32_hal::usbd::Instance;

use {ch32_hal as hal, panic_halt as _};

use crate::ws2812::Ws2812;
use smart_leds::{SmartLedsWrite, RGB8};
use ws2812_spi as ws2812;

mod vendor_class;
use crate::vendor_class::{VendorClass, VendorReceiver, VendorSender};

#[cfg(feature = "storage-flash")]
mod flash_storage;

use light_machine::builder::{MachineBuilderError, Op, ProgramBuilder};
use light_machine::Word;
use pliot::Pliot;
#[cfg(feature = "storage-mem")]
use pliot::meme_storage::MemStorage;
#[cfg(feature = "storage-flash")]
use crate::flash_storage::FlashStorage;

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
const NUM_LEDS: usize = 25;
const FRAME_TARGET: u64 = 16;

#[cfg(all(feature = "storage-mem", feature = "storage-flash"))]
compile_error!("Enable only one of `storage-mem` or `storage-flash` features.");
#[cfg(not(any(feature = "storage-mem", feature = "storage-flash")))]
compile_error!("Enable exactly one of `storage-mem` or `storage-flash` features.");

static PROGRAM_BUFFER: StaticCell<[u16; 100]> = StaticCell::new();
static GLOBALS: StaticCell<[u16; 10]> = StaticCell::new();
#[cfg(feature = "storage-mem")]
static MEM_STORAGE: StaticCell<MemStorage<'static>> = StaticCell::new();
#[cfg(feature = "storage-flash")]
static FLASH_STORAGE: StaticCell<FlashStorage> = StaticCell::new();
static USB_RECEIVE_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static RAW_MESSAGE_BUFF: StaticCell<Vec<u8, INCOMING_MESSAGE_CAP>> = StaticCell::new();
static USB_CONFIG_DESCRIPTOR: StaticCell<[u8; 64]> = StaticCell::new();
static USB_BOS_DESCRIPTOR: StaticCell<[u8; 64]> = StaticCell::new();
static USB_CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static LED_BUFFER: StaticCell<[RGB8; NUM_LEDS]> = StaticCell::new();
static PLIOT_SHARED: StaticCell<Mutex<CriticalSectionRawMutex, PliotShared>> = StaticCell::new();

#[cfg(feature = "storage-mem")]
type StorageImpl = MemStorage<'static>;
#[cfg(feature = "storage-flash")]
type StorageImpl = FlashStorage;

struct PliotShared {
    pliot: Pliot<'static, 'static, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, StorageImpl>,
    stack: Vec<Word, 100>,
}

static mut DEBUG_WS: *mut Ws2812<Spi<'static, peripherals::SPI1, hal::mode::Blocking>> =
    core::ptr::null_mut();
static mut DEBUG_DATA: *mut [RGB8; NUM_LEDS] = core::ptr::null_mut();
static mut DEBUG_STEP: usize = 0;

pub(crate) fn debug_led_init(
    ws: &mut Ws2812<Spi<'static, peripherals::SPI1, hal::mode::Blocking>>,
    data: &mut [RGB8; NUM_LEDS],
) {
    unsafe {
        DEBUG_WS = ws;
        DEBUG_DATA = data;
        DEBUG_STEP = 0;
        if let Some(buf) = DEBUG_DATA.as_mut() {
            buf.fill(RGB8::default());
        }
    }
}

pub(crate) fn debug_led_clear() {
    unsafe {
        DEBUG_WS = core::ptr::null_mut();
        DEBUG_DATA = core::ptr::null_mut();
    }
}

pub(crate) fn debug_mark(color: RGB8) {
    unsafe {
        if DEBUG_WS.is_null() || DEBUG_DATA.is_null() {
            return;
        }
        if DEBUG_STEP >= NUM_LEDS {
            return;
        }
        let data = &mut *DEBUG_DATA;
        data[DEBUG_STEP] = color;
        let _ = (*DEBUG_WS).write(data.clone());
        DEBUG_STEP += 1;
    }
}

#[embassy_executor::main(entry = "qingke_rt::entry")]
async fn main(spawner: Spawner) -> () {
    #[cfg(debug_assertions)]
    hal::debug::SDIPrint::enable();
    let config = hal::Config{ rcc: hal::rcc::Config::SYSCLK_FREQ_144MHZ_HSI, ..Default::default() };
    let p = hal::init(config);

    // SPI1 for WS2812 output.
    let mosi = p.PA7;
    let mut spi_config = hal::spi::Config::default();
    spi_config.frequency = Hertz::mhz(3);
    spi_config.mode = ws2812::MODE;
    let spi = Spi::new_blocking_txonly_nosck(p.SPI1, mosi, spi_config);
    let mut ws = Ws2812::new(spi);
    let data = LED_BUFFER.init([RGB8::default(); NUM_LEDS]);
    debug_led_init(&mut ws, data);

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
    let config_descriptor = USB_CONFIG_DESCRIPTOR.init([0; 64]);
    let bos_descriptor = USB_BOS_DESCRIPTOR.init([0; 64]);
    let control_buf = USB_CONTROL_BUF.init([0; 64]);
    let mut builder = Builder::new(
        driver,
        config,
        config_descriptor,
        bos_descriptor,
        &mut [], // no msos descriptors
        control_buf,
    );

    // Create classes on the builder.
    let class = VendorClass::new(&mut builder, 64);

    // Build the builder.
    let usb = builder.build();

    /////////////////////////////////////////////////
    let globals = GLOBALS.init([0u16; 10]);
    #[cfg(feature = "storage-mem")]
    let storage = {
        let program_buffer = PROGRAM_BUFFER.init([0u16; 100]);
        if get_program(program_buffer).is_err() {
            // BUG: we should log here.
            return;
        }
        MEM_STORAGE.init(MemStorage::new(program_buffer.as_mut_slice()))
    };
    #[cfg(feature = "storage-flash")]
    let storage = {
        use pliot::StorageError;

        let mut storage = match FlashStorage::open() {
            Ok(storage) => storage,
            Err(e) =>  match e {   
                StorageError::InvalidHeader => {
                    if FlashStorage::probe_write_read().is_err() {
                        // BUG: we should log here.
                        return;
                    }
                    if FlashStorage::format().is_err() {
                        // BUG: we should log here.
                        return;
                    }
                    let Ok(storage) = FlashStorage::open() else  {
                         // BUG: we should log here.
                        return;
                    };

                    storage
                }
                _ => {
                    // BUG: we should log here.
                    return;
                }
            }
        };
        if storage.is_empty() {
            let program_buffer = PROGRAM_BUFFER.init([0u16; 100]);
            let program_len = match get_program(program_buffer) {
                Ok(length) => length,
                Err(_) => {
                    // BUG: we should log here.
                    return;
                }
            };
            let Some(program) = program_buffer.get(..program_len) else {
                // BUG: we should log here.
                return;
            };
            if storage.write_program(program).is_err() {
                // BUG: we should log here.
                return;
            }
        }
        FLASH_STORAGE.init(storage)
    };

    let shared = PLIOT_SHARED.init(Mutex::new(PliotShared {
        pliot: Pliot::new(storage, globals.as_mut_slice()),
        stack: Vec::new(),
    }));
    let (usb_sender, usb_receiver) = class.split();

    ////////////////////////////////////////////

    // BUG: we should check the values and log + restart
    // If we cant't start these the device will not work.
    let _ = spawner.spawn(usb_device_task(usb));
    let _ = spawner.spawn(io_task(usb_receiver, usb_sender, shared));
    // Boot step 2: USB tasks spawned.
    debug_led_clear();
    led_task(
            &mut ws,
            data,
            shared,
        ).await;
}

#[embassy_executor::task]
async fn usb_device_task(
    mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USBD>>,
) {
    usb.run().await;
}

#[embassy_executor::task]
async fn io_task(
    mut receiver: VendorReceiver<'static, Driver<'static, peripherals::USBD>>,
    mut sender: VendorSender<'static, Driver<'static, peripherals::USBD>>,
    shared: &'static Mutex<CriticalSectionRawMutex, PliotShared>,
) {
    loop {
        receiver.wait_connection().await;
        sender.wait_connection().await;
        let _ = io_loop(&mut receiver, &mut sender, shared).await;
    }
}

async fn led_task(
    ws: &mut Ws2812<Spi<'static, peripherals::SPI1, hal::mode::Blocking>>,
    data: &mut [RGB8; NUM_LEDS],
    shared: &'static Mutex<CriticalSectionRawMutex, PliotShared>,
) {
    loop {
        // If I could track times I could make my anamations
        // have really tight timeing when there is no message
        // to process but this makes the program a little too
        // large. If I can find some spae else where lets try
        // this agin.
        let start_time = Instant::now();
        {
            let mut guard = shared.lock().await;
            let PliotShared { pliot, stack } = &mut *guard;
            for (i, led) in data.iter_mut().enumerate() {
                if let Ok((red, green, blue)) = pliot.get_led_color(0, i as u16, stack) {
                    *led = (red, green, blue).into();
                    stack.clear();
                }
            }
        }


        let _ = ws.write(data.clone());
        
        let wait_duration = match Duration::from_millis(FRAME_TARGET).checked_sub(start_time.elapsed()) {
            Some(d) => d,
            None => Duration::from_millis(0),
        };

        Timer::after(wait_duration).await;
    }
}


#[link_section = ".coldtext"]
#[inline(never)]
fn get_program(buffer: &mut [u16]) -> Result<usize, MachineBuilderError> {
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
    function.add_op(Op::Exit)?;
    let (_function_index, machine) = function.finish()?;

    let mut function = machine.new_function()?;
    function.add_op(Op::Load(0))?;
    function.add_op(Op::Load(1))?;
    function.add_op(Op::Load(2))?;
    function.add_op(Op::Exit)?;
    let (_function_index, machine) = function.finish()?;

    let program_builder = machine.finish()?;
    let descriptor = program_builder.finish_program();

    Ok(descriptor.length)
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

async fn io_loop<'d, T: Instance + 'd>(
    receiver: &mut VendorReceiver<'d, Driver<'d, T>>,
    sender: &mut VendorSender<'d, Driver<'d, T>>,
    shared: &'static Mutex<CriticalSectionRawMutex, PliotShared>,
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
                let mut out_buf = [0u8; OUTGOING_MESSAGE_CAP];
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
                            // BUG: we should log error
                            stack.clear();
                            0
                        }
                    }
                };
                frame.clear();

                if wrote > 0 {
                    if let Some(bytes) = out_buf.get(..wrote) {
                        sender.write_packet(bytes).await?;
                    } else {
                        //BUG: this should be unreachable but we should log
                        // to catch bugs introduced in refactoring.
                    }
                }
            }
        }
    }
}
