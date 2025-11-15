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
use embassy_sync::channel::{Channel, Sender};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;


use ch32_hal::bind_interrupts;
use ch32_hal::usbd::Driver;
use ch32_hal::usbd::Instance;

use {ch32_hal as hal, panic_halt as _};

use crate::ws2812::Ws2812;
use smart_leds::{brightness, SmartLedsWrite, RGB8};
use ws2812_spi as ws2812;

use core::sync::atomic::AtomicU8;
use core::sync::atomic::Ordering;
use light_machine::{
    builder::{
        MachineBuilderError,
        ProgramBuilder,
        Op,
    },
    MachineError, 
    Program,
    Word,
};

use heapless::Vec;

bind_interrupts!(struct Irqs {
    USB_LP_CAN1_RX0 => ch32_hal::usbd::InterruptHandler<peripherals::USBD>;
});

const MODE_CYCLE: u8 = 0;
const MODE_RED: u8 = 1;
const MODE_GREEN: u8 = 2;
const MODE_BLUE: u8 = 3;


static CHANNEL: Channel::<CriticalSectionRawMutex, u8, 1> = Channel::new();

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
    let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    /////////////////////////////////////////////////


    let mut buffer = [0u16; 100];
    get_program(&mut buffer).expect("could not get program");
    let mut globals = [0u16; 10];
    let mut stack: Vec<Word, 100> = Vec::new();
    let mut program = Program::new(buffer.as_slice(), globals.as_mut_slice()).expect("clould not init program");

    let sender = CHANNEL.sender();


    // Do stuff with the class!
    let echo_fut = async {
        loop {
            class.wait_connection().await;
            let _ = echo(&mut class, sender).await;
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
        let mut mode = b'c';
        loop {
            for j in 0..(256 * 5) {
                for i in 0..NUM_LEDS {
                    
                    if let Ok(read_mode) = CHANNEL.try_receive() {
                        mode = read_mode;
                        match mode {
                            b'r' => {
                                stack.push(128).unwrap();
                                stack.push(0).unwrap();
                                stack.push(0).unwrap();

                                program.init_machine(0, &mut stack).unwrap();
                            }
                            b'g' => {//MODE_GREEN => {
                                stack.push(0).unwrap();
                                stack.push(128).unwrap();
                                stack.push(0).unwrap();

                                program.init_machine(0, &mut stack).unwrap();
                            }
                            b'b' => {//MODE_BLUE => {
                                stack.push(0).unwrap();
                                stack.push(0).unwrap();
                                stack.push(128).unwrap();

                                program.init_machine(0, &mut stack).unwrap();
                            }
                            _ => {
                                stack.push(128).unwrap();
                                stack.push(0).unwrap();
                                stack.push(0).unwrap();

                                program.init_machine(0, &mut stack).unwrap();
                            }
                        }
                    }
                    //let mode = MODE.load(Ordering::Relaxed);
                    match mode {
                        b'c' => { //MODE_CYCLE => {
                            data[i] = wheel(
                                (((i * 256) as u16 / NUM_LEDS as u16 + j as u16) & 255) as u8,
                            );
                        }
                        _ => {
                            if let Ok((red, green, blue)) = program.get_led_color(0, i as u16, &mut stack) {
                                data[i] = (red, green, blue).into();
                                stack.clear();
                            } else {
                                // ???
                            }
                        }
                    }
                }
                ws.write(brightness(data.iter().cloned(), 2)).unwrap();
                Timer::after_millis(10).await;
            }
        }
    };

    // Run everything concurrently.
    // If we had made everything `'static` above instead, we could do this using separate tasks instead.
    join3(usb_fut, echo_fut, led_loop).await;
}


fn get_program(buffer: &mut [u16])-> Result<(), MachineBuilderError>{
    let machine_count = 1;
    let program_builder = ProgramBuilder::new(buffer, machine_count)?;

    let function_count = 2;
    let globals_size = 3;
    let machine = program_builder.new_machine(function_count, globals_size)?;
    let mut function = machine.new_function()?;
    function.add_op(Op::Store(0))?;
    function.add_op(Op::Store(1))?;
    function.add_op(Op::Store(2))?;
    function.add_op(Op::Return)?;
    let (_function_index, machine) = function.finish();

    let mut function = machine.new_function()
        .expect("could not get fucntion builder");
    function.add_op(Op::Load(0))?;
    function.add_op(Op::Load(1))?;
    function.add_op(Op::Load(2))?;
    function.add_op(Op::Return)?;
    let (_function_index,machine) = function.finish();

    let _program_builder = machine.finish();

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

async fn echo<'d, T: Instance + 'd>(
    class: &mut CdcAcmClass<'d, Driver<'d, T>>,
    tx: Sender<'static, CriticalSectionRawMutex, u8, 1>
) -> Result<(), Disconnected> {
    let mut buf = [0; 64];
    loop {
        let n = class.read_packet(&mut buf).await?;
        let data = &buf[..n];
        if data.len() > 0 {
            // we don't care if were full just drop it
            let _ = tx.try_send(data[0]);  
            let mode = match data[0] {
                b'c' => MODE_CYCLE,
                b'r' => MODE_RED,
                b'g' => MODE_GREEN,
                b'b' => MODE_BLUE,
                _ => MODE_RED,
            };
        };
        //class.write_packet(data).await?;
    }
}

