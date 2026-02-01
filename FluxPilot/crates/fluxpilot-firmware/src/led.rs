use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use heapless::Vec;
use light_machine::StackWord;
use smart_leds::{SmartLedsWrite, RGB8};

use crate::usb_io::PliotShared;

pub async fn led_loop<
    W,
    S,
    const MAX_ARGS: usize,
    const MAX_RESULT: usize,
    const PROGRAM_BLOCK_SIZE: usize,
    const UI_BLOCK_SIZE: usize,
    const STACK_SIZE: usize,
    const NUM_LEDS: usize,
    const FRAME_TARGET_MS: u64,
>(
    writer: &mut W,
    data: &mut [RGB8; NUM_LEDS],
    shared: &'static Mutex<
        CriticalSectionRawMutex,
        PliotShared<
            'static,
            'static,
            S,
            MAX_ARGS,
            MAX_RESULT,
            PROGRAM_BLOCK_SIZE,
            UI_BLOCK_SIZE,
            STACK_SIZE,
        >,
    >,
) where
    W: SmartLedsWrite<Color = RGB8>,
    S: pliot::Storage,
{
    let mut tick = 0u16;
    loop {
        let start_time = Instant::now();
        {
            let mut guard = shared.lock().await;
            let PliotShared { pliot, stack } = &mut *guard;
            let machine_count = match pliot.machine_count() {
                Ok(count) => count,
                Err(_) => {
                    return;
                }
            };
            let seed_stack =
                |stack: &mut Vec<StackWord, STACK_SIZE>, red: u8, green: u8, blue: u8| -> bool {
                    stack.clear();
                    if stack.push(red as StackWord).is_err() {
                        return false;
                    }
                    if stack.push(green as StackWord).is_err() {
                        stack.clear();
                        return false;
                    }
                    if stack.push(blue as StackWord).is_err() {
                        stack.clear();
                        return false;
                    }
                    true
                };
            for (i, led) in data.iter_mut().enumerate() {
                let mut red = 0u8;
                let mut green = 0u8;
                let mut blue = 0u8;
                if !seed_stack(stack, red, green, blue) {
                    continue;
                }
                for machine_number in 0..machine_count {
                    match pliot.get_led_color(machine_number, i as u16, tick, stack) {
                        Ok((next_red, next_green, next_blue)) => {
                            red = next_red;
                            green = next_green;
                            blue = next_blue;
                            if !seed_stack(stack, red, green, blue) {
                                break;
                            }
                        }
                        Err(_) => {
                            stack.clear();
                            break;
                        }
                    }
                }
                *led = (red, green, blue).into();
            }
        }

        let _ = writer.write(data.clone());

        let wait_duration = match Duration::from_millis(FRAME_TARGET_MS)
            .checked_sub(start_time.elapsed())
        {
            Some(d) => d,
            None => Duration::from_millis(0),
        };

        Timer::after(wait_duration).await;
        tick = tick.wrapping_add(1);
    }
}
