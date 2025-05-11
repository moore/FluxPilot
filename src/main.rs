#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use qingke::riscv;


use hal::gpio::{AnyPin, Level, Output, Pin};
use hal::prelude::*;
use hal::spi::Spi;
use hal::{peripherals, println};
use {ch32_hal as hal, panic_halt as _};
use smart_leds::{brightness, SmartLedsWrite, RGB8};
use ws2812_spi as ws2812;
use crate::ws2812::Ws2812;


#[qingke_rt::entry]
fn main() -> ! {
    hal::debug::SDIPrint::enable();
    let mut config = hal::Config::default();
    config.rcc = hal::rcc::Config::SYSCLK_FREQ_144MHZ_HSI;
    let p = hal::init(config);

    // SPI1, remap 0
    let sck = p.PA5;
    let MOSI = p.PA7;

    let rst = p.PB0;


    let mut rst = Output::new(rst, Level::High, Default::default());


    let mut spi_config = hal::spi::Config::default();
    spi_config.frequency = Hertz::mhz(3);
    spi_config.mode = ws2812::MODE;

    let spi = Spi::new_blocking_txonly(p.SPI1, sck, MOSI, spi_config);

    rst.set_low();
    riscv::asm::delay(120_000_000);
    rst.set_high();
    riscv::asm::delay(20_000_000);

    let mut ws = Ws2812::new(spi);

   

    const NUM_LEDS: usize = 25;
    let mut data = [RGB8::default(); NUM_LEDS];

    loop {
        for j in 0..(256 * 5) {
            for i in 0..NUM_LEDS {
                data[i] = wheel((((i * 256) as u16 / NUM_LEDS as u16 + j as u16) & 255) as u8);
            }
            ws.write(brightness(data.iter().cloned(), 32)).unwrap();
            riscv::asm::delay(500_000);
        }
    }
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