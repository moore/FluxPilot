#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use hal::peripherals;
use hal::prelude::*;
use hal::spi::Spi;

use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_time::{Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::Builder;

use ch32_hal::bind_interrupts;
use ch32_hal::usbd::Driver;
use ch32_hal::usbd::Instance;

use {ch32_hal as hal, panic_halt as _};

use crate::ws2812::Ws2812;
use smart_leds::{brightness, SmartLedsWrite, RGB8};
use ws2812_spi as ws2812;

use core::sync::atomic::AtomicU8;
use core::sync::atomic::Ordering;

use light_machine;

bind_interrupts!(struct Irqs {
    USB_LP_CAN1_RX0 => ch32_hal::usbd::InterruptHandler<peripherals::USBD>;
});

static MODE: AtomicU8 = AtomicU8::new(0);
const MODE_CYCLE: u8 = 0;
const MODE_RED: u8 = 1;
const MODE_GREEN: u8 = 2;
const MODE_BLUE: u8 = 3;

#[embassy_executor::main(entry = "qingke_rt::entry")]
async fn main(_spawner: Spawner) {
    hal::debug::SDIPrint::enable();
    let mut config = hal::Config::default();
    config.rcc = hal::rcc::Config::SYSCLK_FREQ_144MHZ_HSI;
    let p = hal::init(config);

    let usb: peripherals::USBD = p.USBD;

    /* USB DRIVER SECION */
    let driver = Driver::new(usb, Irqs, p.PA12, p.PA11);

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0x4348, 0x55e0);
    config.manufacturer = Some("0xa9f4");
    config.product = Some("FluxPilot");
    config.serial_number = Some("314159");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Windows compatibility requires these; CDC-ACM
    //config.device_class = 0x02;
    // But we sue 0xff becouse we need it to not be bound
    // by Android to work on mobil chome.
    config.device_class = 0xFF;
    config.device_sub_class = 0x02;
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
    let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    // Do stuff with the class!
    let echo_fut = async {
        loop {
            class.wait_connection().await;
            let _ = echo(&mut class).await;
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

    let led_loop = async {
        loop {
            for j in 0..(256 * 5) {
                for i in 0..NUM_LEDS {
                    let mode = MODE.load(Ordering::Relaxed);
                    match mode {
                        MODE_CYCLE => {
                            data[i] = wheel(
                                (((i * 256) as u16 / NUM_LEDS as u16 + j as u16) & 255) as u8,
                            );
                        }
                        MODE_RED => {
                            data[i] = (128, 0, 0).into();
                        }
                        MODE_GREEN => {
                            data[i] = (0, 128, 0).into();
                        }
                        MODE_BLUE => {
                            data[i] = (0, 0, 128).into();
                        }
                        _ => {
                            data[i] = (128, 0, 128).into();
                        }
                    }
                }
                ws.write(brightness(data.iter().cloned(), 32)).unwrap();
                Timer::after(Duration::from_millis(10)).await;
            }
        }
    };

    // Run everything concurrently.
    // If we had made everything `'static` above instead, we could do this using separate tasks instead.
    join3(usb_fut, echo_fut, led_loop).await;
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

async fn echo<'d, T: Instance + 'd>(
    class: &mut CdcAcmClass<'d, Driver<'d, T>>,
) -> Result<(), Disconnected> {
    let mut buf = [0; 64];
    loop {
        let n = class.read_packet(&mut buf).await?;
        let data = &buf[..n];
        if data.len() > 0 {
            let mode = match data[0] {
                b'c' => MODE_CYCLE,
                b'r' => MODE_RED,
                b'g' => MODE_GREEN,
                b'b' => MODE_BLUE,
                _ => MODE_RED,
            };
            MODE.store(mode, Ordering::Relaxed);
        };
        //class.write_packet(data).await?;
    }
}


////// Temp Code ///////

struct MachineData {
    globals: [Word; 10],
    static_data: [Word; 0],
    program: [Word; 100],
    main: usize,
    init: usize,
}

fn get_test_program() -> MachineData {
    let globals = [0u16; 10];
    let static_data = [0u16; 0];
    let mut program = [0u16; 100];

    // main
    // globals, 0, 1, 2, on to stack
    program[0] = Ops::Load.into();
    program[1] = 0;
    program[2] = Ops::Load.into();
    program[3] = 1;
    program[4] = Ops::Load.into();
    program[5] = 2;
    program[6] = Ops::Return.into();

    // init stor the top three entires in to globals 0, 1, 2
    program[7] = Ops::Store.into();
    program[8] = 0;
    program[9] = Ops::Store.into();
    program[10] = 1;
    program[11] = Ops::Store.into();
    program[12] = 2;
    program[13] = Ops::Return.into();

    MachineData {
        globals,
        static_data,
        program,
        main: 0,
        init: 7,
    }
}