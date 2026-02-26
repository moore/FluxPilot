#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

mod board;

use embassy_rp as hal;
use embassy_rp::block::ImageDef;

use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals;
use embassy_rp::flash;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c;
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{Grb, PioWs2812, PioWs2812Program};
use embassy_rp::usb;
use embassy_rp::watchdog::{ResetReason, Watchdog};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use embassy_usb::Builder;
use heapless::Vec;
use static_cell::StaticCell;
use core::sync::atomic::{AtomicBool, Ordering};
use smart_leds::RGB8;
use light_machine::StackWord;

use fluxpilot_firmware::program::default_program;
use fluxpilot_firmware::usb_io::{io_loop, PliotShared};
use fluxpilot_firmware::usb_vendor::{VendorClass, VendorReceiver, VendorSender};
use fluxpilot_firmware::flash_storage::FlashStorage;
use pliot::protocol::FunctionId;
use pliot::Pliot;

mod build_constants {
    include!(concat!(env!("OUT_DIR"), "/memory_consts.rs"));
}

use build_constants::FLASH_SIZE;


// Panic handler
use panic_probe as _;
// Defmt Logging
use defmt_rtt as _;

/// Tell the Boot ROM about our application
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = hal::block::ImageDef::secure_exe();

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<peripherals::USB>;
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<peripherals::PIO0>;
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<peripherals::I2C0>;
});

const MAX_ARGS: usize = 3;
const MAX_RESULT: usize = 3;
const PROGRAM_BLOCK_SIZE: usize = 64;
const UI_BLOCK_SIZE: usize = 128;
const INCOMING_MESSAGE_CAP: usize = 2048;
const OUTGOING_MESSAGE_CAP: usize = 1048;
const NUM_LEDS: usize = 1024;
const FRAME_TARGET_MS: u64 = 16;
const PROGRAM_BUFFER_SIZE: usize = 1024;
const USB_RECEIVE_BUF_SIZE: usize = 2048;
const I2C_RECEIVE_BUF_SIZE: usize = 16;
const I2C_SCAN_START: u8 = 0x08;
const I2C_SCAN_END: u8 = 0x77;
const I2C_MAX_DEVICES: usize = 16;
const I2C_READ_LEN: usize = 16;
const I2C_READ_INTERVAL_MS: u64 = 50;
const I2C_ROUTE_REFRESH_MS: u64 = 1_000;
const I2C_ROUTE_BUS_ID: u8 = 0;
const I2C_ROUTE_GET_ROUTES_FUNCTION_ID: u32 = 1;
const I2C_ROUTE_MAX_ENTRIES: usize = 16;
const I2C_ROUTE_MAX_TARGETS_PER_ENTRY: usize = 8;
const WATCHDOG_RESET_THRESHOLD: u32 = 3;
const WATCHDOG_PERIOD_MS: u64 = 2_000;
const WATCHDOG_FEED_MS: u64 = 500;
const RUNTIME_MEMORY_WORDS: usize = 4096; // 16 KiB total runtime memory (StackWord cells).
const WATCHDOG_SCRATCH_MAGIC: u32 = u32::from_le_bytes(*b"WDT0");

#[repr(align(4))]
struct RuntimeMemory {
    words: [StackWord; RUNTIME_MEMORY_WORDS],
}

type FlashDriver = flash::Flash<'static, peripherals::FLASH, flash::Blocking, FLASH_SIZE>;
type StorageImpl = FlashStorage<FlashDriver>;
type SharedState = PliotShared<
    'static,
    'static,
    StorageImpl,
    MAX_ARGS,
    MAX_RESULT,
    PROGRAM_BLOCK_SIZE,
    UI_BLOCK_SIZE,
>;

static PROGRAM_BUFFER: StaticCell<[u16; PROGRAM_BUFFER_SIZE]> = StaticCell::new();
static RUNTIME_MEMORY: StaticCell<RuntimeMemory> = StaticCell::new();
static FLASH_STORAGE: StaticCell<FlashStorage<FlashDriver>> = StaticCell::new();
static USB_RECEIVE_BUF: StaticCell<[u8; USB_RECEIVE_BUF_SIZE]> = StaticCell::new();
static I2C_RECEIVE_BUF: StaticCell<[u8; I2C_RECEIVE_BUF_SIZE]> = StaticCell::new();
static RAW_MESSAGE_BUFF: StaticCell<Vec<u8, INCOMING_MESSAGE_CAP>> = StaticCell::new();
static USB_CONFIG_DESCRIPTOR: StaticCell<[u8; 64]> = StaticCell::new();
static USB_BOS_DESCRIPTOR: StaticCell<[u8; 64]> = StaticCell::new();
static USB_CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static LED_BUFFER: StaticCell<[RGB8; NUM_LEDS]> = StaticCell::new();
static PLIOT_SHARED: StaticCell<Mutex<CriticalSectionRawMutex, SharedState>> = StaticCell::new();
static USB_CONNECTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct I2cRouteTarget {
    machine_index: u16,
    function_index: u32,
}

struct I2cRouteEntry {
    address_7bit: u8,
    targets: Vec<I2cRouteTarget, I2C_ROUTE_MAX_TARGETS_PER_ENTRY>,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = embassy_rp::init(Default::default());
    let mut watchdog = Watchdog::new(p.WATCHDOG);
    let reset_reason = watchdog.reset_reason();
    let mut reset_count = if watchdog.get_scratch(1) == WATCHDOG_SCRATCH_MAGIC {
        watchdog.get_scratch(0)
    } else {
        0
    };
    if reset_reason == Some(ResetReason::TimedOut) {
        reset_count = reset_count.saturating_add(1);
    } else {
        reset_count = 0;
    }
    watchdog.set_scratch(1, WATCHDOG_SCRATCH_MAGIC);
    watchdog.set_scratch(0, reset_count);
    let clear_program = reset_count >= WATCHDOG_RESET_THRESHOLD;
    if clear_program {
        watchdog.set_scratch(0, 0);
    }

    let mut pio = Pio::new(p.PIO0, Irqs);
    let program = PioWs2812Program::new(&mut pio.common);
    if board::LED_DATA_3V3_GPIO != 15 {
        panic!("LED data pin must be GPIO15 for PIO WS2812");
    }
    let mut led_driver = PioWs2812::<_, 0, NUM_LEDS, Grb>::new(
        &mut pio.common,
        pio.sm0,
        p.DMA_CH0,
        p.PIN_15,
        &program,
    );

    if board::LED_B_GPIO != 18 {
        panic!("LED blue pin must be GPIO18 for heartbeat");
    }
    let onboard_blue = Output::new(p.PIN_18, Level::High);

    let data = LED_BUFFER.init([RGB8::default(); NUM_LEDS]);
    let usb = p.USB;
    let driver = usb::Driver::new(usb, Irqs);

    //let mut config = embassy_usb::Config::new(0x2E8A, 0x000A);
    let mut config = embassy_usb::Config::new(0x4348, 0x55e0);

    config.manufacturer = Some("Pimoroni");
    config.product = Some("Plasma 2350");
    config.serial_number = Some("plasma2350");
    config.device_class = 0xFF;
    config.device_sub_class = 0x00;
    config.device_protocol = 0x00;
    config.composite_with_iads = false;

    let config_descriptor = USB_CONFIG_DESCRIPTOR.init([0; 64]);
    let bos_descriptor = USB_BOS_DESCRIPTOR.init([0; 64]);
    let control_buf = USB_CONTROL_BUF.init([0; 64]);
    let mut builder = Builder::new(
        driver,
        config,
        config_descriptor,
        bos_descriptor,
        &mut [],
        control_buf,
    );
    let class = VendorClass::new(&mut builder, 64);
    let usb = builder.build();

    let memory = RUNTIME_MEMORY.init(RuntimeMemory {
        words: [0u32; RUNTIME_MEMORY_WORDS],
    });
    let program_buffer = PROGRAM_BUFFER.init([0u16; PROGRAM_BUFFER_SIZE]);
    let storage = {
        use pliot::StorageError;

        let flash = FlashDriver::new_blocking(p.FLASH);
        let flash_base = flash::FLASH_BASE as usize;
        let mut storage = match FlashStorage::new(flash, flash_base) {
            Ok(storage) => storage,
            Err(_) => {
                panic!("flash storage init failed");
            }
        };
        //storage.format(); // Uncomment if needed to clear stored program
        if clear_program && storage.format().is_err() {
            panic!("flash storage watchdog reset format failed");
        }
        match storage.load_header() {
            Ok(()) => {}
            Err(StorageError::InvalidHeader { location: _ }) => {
                if storage.probe_write_read().is_err() {
                    panic!("flash storage probe failed");
                }
                if storage.format().is_err() {
                    panic!("flash storage format failed");
                }
                if storage.load_header().is_err() {
                    panic!("flash storage header reload failed");
                }
            }
            Err(_) => {
                panic!("flash storage header load failed");
            }
        }

        if clear_program || storage.is_empty() {
            write_default_program(&mut storage, program_buffer);
        }
        FLASH_STORAGE.init(storage)
    };
    let shared = PLIOT_SHARED.init(Mutex::new(PliotShared {
        pliot: Pliot::new(storage, memory.words.as_mut_slice()),
    }));

    {
        let mut guard = shared.lock().await;
        let PliotShared { pliot } = &mut *guard;
        if let Err(err) = pliot.init() {
            panic!("pliot init failed '{:?}'", err);
        }
    }

    let (usb_sender, usb_receiver) = class.split();

    let _ = spawner.spawn(usb_device_task(usb));
    let _ = spawner.spawn(io_task(usb_receiver, usb_sender, shared));
    let _ = spawner.spawn(i2c_task(p.I2C0, p.PIN_21, p.PIN_20, shared));
    let _ = spawner.spawn(heartbeat_task(onboard_blue));
    let _ = spawner.spawn(watchdog_task(watchdog));

    led_loop_pio::<
        _,
        _,
        0,
        NUM_LEDS,
        MAX_ARGS,
        MAX_RESULT,
        PROGRAM_BLOCK_SIZE,
        UI_BLOCK_SIZE,
        FRAME_TARGET_MS,
    >(&mut led_driver, data, shared)
    .await;
}


#[embassy_executor::task]
async fn usb_device_task(
    mut usb: embassy_usb::UsbDevice<'static, usb::Driver<'static, peripherals::USB>>,
) {
    usb.run().await;
}

#[embassy_executor::task]
async fn io_task(
    mut receiver: VendorReceiver<'static, usb::Driver<'static, peripherals::USB>>,
    mut sender: VendorSender<'static, usb::Driver<'static, peripherals::USB>>,
    shared: &'static Mutex<CriticalSectionRawMutex, SharedState>,
) {
    let usb_buf = USB_RECEIVE_BUF.init([0u8; USB_RECEIVE_BUF_SIZE]);
    let frame = RAW_MESSAGE_BUFF.init(Vec::new());
    loop {
        USB_CONNECTED.store(false, Ordering::Relaxed);
        receiver.wait_connection().await;
        sender.wait_connection().await;
        USB_CONNECTED.store(true, Ordering::Relaxed);
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
        >(
            &mut receiver,
            &mut sender,
            shared,
            usb_buf,
            frame,
        )
        .await;
        USB_CONNECTED.store(false, Ordering::Relaxed);
    }
}

#[embassy_executor::task]
async fn i2c_task(
    i2c: embassy_rp::Peri<'static, peripherals::I2C0>,
    i2c_scl: embassy_rp::Peri<'static, peripherals::PIN_21>,
    i2c_sda: embassy_rp::Peri<'static, peripherals::PIN_20>,
    shared: &'static Mutex<CriticalSectionRawMutex, SharedState>,
) {
    let mut i2c = i2c::I2c::new_async(
        i2c,
        i2c_scl,
        i2c_sda,
        Irqs,
        i2c::Config::default(),
    ); 

    let i2c_buf = I2C_RECEIVE_BUF.init([0u8; I2C_RECEIVE_BUF_SIZE]);
    let mut routes: Vec<I2cRouteEntry, I2C_ROUTE_MAX_ENTRIES> = Vec::new();

    let mut devices: Vec<u8, I2C_MAX_DEVICES> = Vec::new();
    let probe = [0u8; 1];

    for addr in I2C_SCAN_START..=I2C_SCAN_END {
        if i2c.write_async(addr, probe).await.is_ok() {
            if devices.push(addr).is_err() {
                break;
            }
        }
        Timer::after_millis(1).await;
    }
    {
        let mut guard = shared.lock().await;
        let PliotShared { pliot } = &mut *guard;
        pliot.set_i2c_devices(devices.as_slice());
    }

    refresh_i2c_routes(shared, &mut routes).await;
    let mut last_route_refresh = Instant::now();

    loop {
        if last_route_refresh.elapsed() >= Duration::from_millis(I2C_ROUTE_REFRESH_MS) {
            refresh_i2c_routes(shared, &mut routes).await;
            last_route_refresh = Instant::now();
        }

        if devices.is_empty() {
            Timer::after_millis(I2C_READ_INTERVAL_MS).await;
            continue;
        }
        let read_len = I2C_READ_LEN.min(I2C_RECEIVE_BUF_SIZE);
        for idx in 0..devices.len() {
            let addr = devices[idx];
            if i2c
                .read_async(addr, &mut i2c_buf[..read_len])
                .await
                .is_ok()
            {
                let Some(route) = find_i2c_route(&routes, addr) else {
                    Timer::after_millis(I2C_READ_INTERVAL_MS).await;
                    continue;
                };

                let mut args: Vec<u32, MAX_ARGS> = Vec::new();
                for &byte in &i2c_buf[..read_len] {
                    if args.push(byte as u32).is_err() {
                        break;
                    }
                }

                let mut guard = shared.lock().await;
                let PliotShared { pliot } = &mut *guard;
                for target in route.targets.iter() {
                    let function = FunctionId {
                        machine_index: target.machine_index,
                        function_index: target.function_index,
                    };
                    let _ = pliot.call(function, &args);
                }
            }
            Timer::after_millis(I2C_READ_INTERVAL_MS).await;
        }
    }
}

async fn refresh_i2c_routes(
    shared: &'static Mutex<CriticalSectionRawMutex, SharedState>,
    routes: &mut Vec<I2cRouteEntry, I2C_ROUTE_MAX_ENTRIES>,
) {
    let route_words = {
        let mut guard = shared.lock().await;
        let PliotShared { pliot } = &mut *guard;
        let args: Vec<u32, MAX_ARGS> = Vec::new();
        match pliot.call_static(I2C_ROUTE_GET_ROUTES_FUNCTION_ID, &args) {
            Ok(words) => words,
            Err(_) => {
                routes.clear();
                return;
            }
        }
    };

    if !parse_i2c_routes(route_words.as_slice(), routes) {
        routes.clear();
    }
}

fn parse_i2c_routes(
    words: &[u32],
    routes: &mut Vec<I2cRouteEntry, I2C_ROUTE_MAX_ENTRIES>,
) -> bool {
    routes.clear();
    let Some(&entry_count_word) = words.first() else {
        return true;
    };
    let Ok(entry_count) = usize::try_from(entry_count_word) else {
        return false;
    };

    let mut cursor = 1usize;
    for _ in 0..entry_count {
        let Some(&bus_word) = words.get(cursor) else {
            return false;
        };
        cursor += 1;
        let Some(&address_word) = words.get(cursor) else {
            return false;
        };
        cursor += 1;
        let Some(&target_count_word) = words.get(cursor) else {
            return false;
        };
        cursor += 1;

        let Ok(bus_id) = u8::try_from(bus_word) else {
            return false;
        };
        let Ok(address_7bit) = u8::try_from(address_word) else {
            return false;
        };
        let Ok(target_count) = usize::try_from(target_count_word) else {
            return false;
        };

        let mut targets: Vec<I2cRouteTarget, I2C_ROUTE_MAX_TARGETS_PER_ENTRY> = Vec::new();
        for _ in 0..target_count {
            let Some(&machine_word) = words.get(cursor) else {
                return false;
            };
            cursor += 1;
            let Some(&function_word) = words.get(cursor) else {
                return false;
            };
            cursor += 1;

            if bus_id == I2C_ROUTE_BUS_ID {
                let Ok(machine_index) = u16::try_from(machine_word) else {
                    return false;
                };
                if targets
                    .push(I2cRouteTarget {
                        machine_index,
                        function_index: function_word,
                    })
                    .is_err()
                {
                    return false;
                }
            }
        }

        if bus_id == I2C_ROUTE_BUS_ID
            && routes
                .push(I2cRouteEntry {
                    address_7bit,
                    targets,
                })
                .is_err()
        {
            return false;
        }
    }

    cursor == words.len()
}

fn find_i2c_route<'a>(
    routes: &'a Vec<I2cRouteEntry, I2C_ROUTE_MAX_ENTRIES>,
    address_7bit: u8,
) -> Option<&'a I2cRouteEntry> {
    routes.iter().find(|entry| entry.address_7bit == address_7bit)
}

fn write_default_program(
    storage: &mut StorageImpl,
    program_buffer: &mut [u16; PROGRAM_BUFFER_SIZE],
) {
    let program_len = match default_program(program_buffer) {
        Ok(length) => length,
        Err(_) => {
            panic!("default program build failed");
        }
    };
    let Some(program) = program_buffer.get(..program_len) else {
        panic!("default program bounds invalid");
    };
    if storage.write_program(program).is_err() {
        panic!("default program flash write failed");
    }
}

async fn led_loop_pio<
    P,
    S,
    const SM: usize,
    const NUM_LEDS: usize,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
    const FRAME_TARGET_MS: u64,
>(
    writer: &mut PioWs2812<'static, P, SM, NUM_LEDS, Grb>,
    data: &mut [RGB8; NUM_LEDS],
    shared: &'static Mutex<
        CriticalSectionRawMutex,
        PliotShared<'static, 'static, S, MAX_ARGS, MAX_RESULT, PROGRAM_BLOCK_SIZE, UI_BLOCK_SIZE>,
    >,
) -> ! where
    P: embassy_rp::pio::Instance,
    S: pliot::Storage,
{
    loop {
        let start_time = Instant::now();
        let tick = Instant::now().as_millis() as u32;
        {
            let mut guard = shared.lock().await;
            let PliotShared { pliot } = &mut *guard;
            let machine_count = match pliot.machine_count() {
                Ok(count) => count,
                Err(_) => {
                    continue;
                }
            };
            for (i, led) in data.iter_mut().enumerate() {
                let mut red = 0u8;
                let mut green = 0u8;
                let mut blue = 0u8;
                for machine_number in 0..machine_count {
                    match pliot.get_led_color(machine_number, i as u16, tick, (red, green, blue)) {
                        Ok((next_red, next_green, next_blue)) => {
                            red = next_red;
                            green = next_green;
                            blue = next_blue;
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
                *led = (red, green, blue).into();
            }
        }

        let connected = USB_CONNECTED.load(Ordering::Relaxed);
        if let Some(status) = data.last_mut() {
            if connected {
                *status = (0, 16, 0).into();
            } else if tick & 0x10 == 0 {
                *status = (16, 0, 0).into();
            } else {
                *status = RGB8::default();
            }
        }
        writer.write(data).await;

        let wait_duration = match Duration::from_millis(FRAME_TARGET_MS)
            .checked_sub(start_time.elapsed())
        {
            Some(d) => d,
            None => Duration::from_millis(0),
        };

        Timer::after(wait_duration).await;
    }
}

#[embassy_executor::task]
async fn heartbeat_task(mut led: Output<'static>) {
    loop {
        led.set_low();
        Timer::after_millis(40).await;
        led.set_high();
        Timer::after_millis(960).await;
    }
}

#[embassy_executor::task]
async fn watchdog_task(mut watchdog: Watchdog) {
    watchdog.pause_on_debug(true);
    watchdog.start(Duration::from_millis(WATCHDOG_PERIOD_MS));
    loop {
        Timer::after_millis(WATCHDOG_FEED_MS).await;
        watchdog.feed();
    }
}




// Program metadata for `picotool info`.
// This isn't needed, but it's recommended to have these minimal entries.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"fluxpilot_plasma2350"),
    embassy_rp::binary_info::rp_program_description!(
        c"your program description"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

// End of file
