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
use embassy_usb::Builder;

use ch32_hal::bind_interrupts;
use ch32_hal::usbd::Driver;

use {ch32_hal as hal, panic_halt as _};

use crate::ws2812::Ws2812;
use smart_leds::{SmartLedsWrite, RGB8};
use light_machine::StackWord;
use ws2812_spi as ws2812;

use fluxpilot_firmware::led::led_loop;
use fluxpilot_firmware::program::default_program;
use fluxpilot_firmware::usb_io::{io_loop, PliotShared};
use fluxpilot_firmware::usb_vendor::{VendorClass, VendorReceiver, VendorSender};

#[cfg(feature = "storage-flash")]
mod ch32_flash;

use pliot::Pliot;
#[cfg(feature = "storage-mem")]
use pliot::meme_storage::MemStorage;
#[cfg(feature = "storage-flash")]
use crate::ch32_flash::Ch32Flash;
#[cfg(feature = "storage-flash")]
use fluxpilot_firmware::flash_storage::FlashStorage;

use heapless::Vec;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USB_LP_CAN1_RX0 => ch32_hal::usbd::InterruptHandler<peripherals::USBD>;
});

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 100;
const UI_BLOCK_SIZE: usize = 128;
const INCOMING_MESSAGE_CAP: usize = 128;
const OUTGOING_MESSAGE_CAP: usize = 128;
const NUM_LEDS: usize = 25;
const FRAME_TARGET: u64 = 16;
const PROGRAM_BUFFER_SIZE: usize = 1024;
const UI_STATE_BUFFER_SIZE: usize = 1024;
const USB_RECEIVE_BUF_SIZE: usize = 265; // BUG: I don't know the correct size
const STACK_SIZE: usize = 100;
const GLOBALS_SIZE: usize = 10;
const RUNTIME_MEMORY_WORDS: usize = GLOBALS_SIZE + STACK_SIZE;
#[cfg(feature = "storage-flash")]
const FLASH_BASE: usize = 0x0000_0000;
#[cfg(all(feature = "storage-mem", feature = "storage-flash"))]
compile_error!("Enable only one of `storage-mem` or `storage-flash` features.");
#[cfg(not(any(feature = "storage-mem", feature = "storage-flash")))]
compile_error!("Enable exactly one of `storage-mem` or `storage-flash` features.");

static PROGRAM_BUFFER: StaticCell<[u16; PROGRAM_BUFFER_SIZE]> = StaticCell::new();
static UI_STATE_BUFFER: StaticCell<[u8; UI_STATE_BUFFER_SIZE]> = StaticCell::new();
static RUNTIME_MEMORY: StaticCell<[StackWord; RUNTIME_MEMORY_WORDS]> = StaticCell::new();
#[cfg(feature = "storage-mem")]
static MEM_STORAGE: StaticCell<MemStorage<'static>> = StaticCell::new();
#[cfg(feature = "storage-flash")]
static FLASH_STORAGE: StaticCell<FlashStorage> = StaticCell::new();
static USB_RECEIVE_BUF: StaticCell<[u8; USB_RECEIVE_BUF_SIZE]> = StaticCell::new();
static RAW_MESSAGE_BUFF: StaticCell<Vec<u8, INCOMING_MESSAGE_CAP>> = StaticCell::new();
static USB_CONFIG_DESCRIPTOR: StaticCell<[u8; 64]> = StaticCell::new();
static USB_BOS_DESCRIPTOR: StaticCell<[u8; 64]> = StaticCell::new();
static USB_CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static LED_BUFFER: StaticCell<[RGB8; NUM_LEDS]> = StaticCell::new();
static PLIOT_SHARED: StaticCell<Mutex<CriticalSectionRawMutex, SharedState>> = StaticCell::new();

#[cfg(feature = "storage-mem")]
type StorageImpl = MemStorage<'static>;
#[cfg(feature = "storage-flash")]
type StorageImpl = FlashStorage;

type SharedState =
    PliotShared<
        'static,
        'static,
        StorageImpl,
        MAX_ARGS,
        MAX_RESULT,
        PROGRAM_BLOCK_SIZE,
        UI_BLOCK_SIZE,
    >;

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
        let Some(slot) = data.get_mut(DEBUG_STEP) else {
            return;
        };
        *slot = color;
        let _ = (*DEBUG_WS).write(*data);
        DEBUG_STEP = match DEBUG_STEP.checked_add(1) {
            Some(next) => next,
            None => NUM_LEDS,
        };
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
    let memory = RUNTIME_MEMORY.init([0u32; RUNTIME_MEMORY_WORDS]);
    #[cfg(feature = "storage-mem")]
    let storage = {
        let program_buffer = PROGRAM_BUFFER.init([0u16; PROGRAM_BUFFER_SIZE]);
        if default_program(program_buffer).is_err() {
            // BUG: we should log here.
            return;
        }
        let ui_state_buffer = UI_STATE_BUFFER.init([0u8; UI_STATE_BUFFER_SIZE]);
        MEM_STORAGE.init(MemStorage::new(
            program_buffer.as_mut_slice(),
            ui_state_buffer.as_mut_slice(),
        ))
    };
    #[cfg(feature = "storage-flash")]
    let storage = {
        use pliot::{StorageError, StorageErrorKind};

        let flash_size = hal::signature::flash_size_kb() as usize * 1024;
        let mut storage = match FlashStorage::new(Ch32Flash::new(FLASH_BASE, flash_size), FLASH_BASE)
        {
            Ok(storage) => storage,
            Err(_) => {
                // BUG: we should log here.
                return;
            }
        };
        match storage.load_header() {
            Ok(()) => {}
            Err(err) if err.kind() == StorageErrorKind::InvalidHeader => {
                if storage.probe_write_read().is_err() {
                    // BUG: we should log here.
                    return;
                }
                if storage.format().is_err() {
                    // BUG: we should log here.
                    return;
                }
                if storage.load_header().is_err() {
                    // BUG: we should log here.
                    return;
                }
            }
            Err(_) => {
                // BUG: we should log here.
                return;
            }
        }
        if storage.is_empty() {
            let program_buffer = PROGRAM_BUFFER.init([0u16; 100]);
            let program_len = match default_program(program_buffer) {
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
        pliot: Pliot::new(storage, memory.as_mut_slice()),
    }));
    {
        let mut guard = shared.lock().await;
        let PliotShared { pliot } = &mut *guard;
        if pliot.init().is_err() {
            debug_mark((32,0,0).into());
            // BUG: add Logging
            return;
        }
    }
    
    let (usb_sender, usb_receiver) = class.split();

    ////////////////////////////////////////////

    // BUG: we should check the values and log + restart
    // If we cant't start these the device will not work.
    let _ = spawner.spawn(usb_device_task(usb));
    let _ = spawner.spawn(io_task(usb_receiver, usb_sender, shared));
    // Boot step 2: USB tasks spawned.
    debug_led_clear();
    led_loop::<
        _,
        _,
        MAX_ARGS,
        MAX_RESULT,
        PROGRAM_BLOCK_SIZE,
        UI_BLOCK_SIZE,
        NUM_LEDS,
        FRAME_TARGET,
    >(&mut ws, data, shared)
    .await;
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
    shared: &'static Mutex<CriticalSectionRawMutex, SharedState>,
) {
    let usb_buf = USB_RECEIVE_BUF.init([0u8; USB_RECEIVE_BUF_SIZE]);
    let frame = RAW_MESSAGE_BUFF.init(Vec::new());
    loop {
        receiver.wait_connection().await;
        sender.wait_connection().await;
        frame.clear();
        let _ = io_loop::<
            _,
            _,
            MAX_ARGS,
            MAX_RESULT,
            PROGRAM_BLOCK_SIZE,
            UI_BLOCK_SIZE,
            USB_RECEIVE_BUF_SIZE,
            INCOMING_MESSAGE_CAP,
            OUTGOING_MESSAGE_CAP,
        >(&mut receiver, &mut sender, shared, usb_buf, frame)
        .await;
    }
}
