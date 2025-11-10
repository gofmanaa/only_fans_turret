#![no_std]
#![no_main]

use arduino_hal::hal::port::Dynamic;
use arduino_hal::port::{
    mode::{Input, Output, PullUp},
    Pin,
};
use arduino_hal::prelude::*;
use arduino_hal::{delay_ms, Peripherals};
use panic_halt as _;
use ufmt::{uwrite, uwriteln};

// --- Remote command definitions ---
const CMD_LEFT: u8 = 0x08;
const CMD_RIGHT: u8 = 0x5A;
const CMD_UP: u8 = 0x52;
const CMD_DOWN: u8 = 0x18;
const CMD_OK: u8 = 0x1C;
const CMD_STAR: u8 = 0x16;

const SERVO_PERIOD_US: u32 = 20_000;
const SERVO_MIN_US: u32 = 1_000;
const SERVO_MAX_US: u32 = 2_000;

const PITCH_MOVE_SPEED: i16 = 8;
const YAW_MOVE_SPEED: i16 = 60;
const ROLL_MOVE_SPEED: i16 = 90;
const YAW_PRECISION_MS: u32 = 100;
const ROLL_PRECISION_MS: u32 = 228;
const PITCH_MAX: i16 = 150;
const PITCH_MIN: i16 = 33;

static mut PITCH_VAL: i16 = 100;
const YAW_STOP: u8 = 90;
const ROLL_STOP: u8 = 90;


type Serial = arduino_hal::hal::usart::Usart0<arduino_hal::DefaultClock>;


fn angle_to_pulse_us(angle: u8) -> u32 {
    let angle = angle as u32;
    (SERVO_MIN_US * (180 - angle) + SERVO_MAX_US * angle) / 180
}

fn servo_pulse(pin: &mut Pin<Output, Dynamic>, pulse_us: u32) {
    pin.set_high();
    arduino_hal::delay_us(pulse_us);
    pin.set_low();
}

fn servo_write_blocking(pin: &mut Pin<Output, Dynamic>, angle: u8, hold_ms: u32) {
    let pulse = angle_to_pulse_us(angle);
    let loops = (hold_ms * 1000) / SERVO_PERIOD_US;
    for _ in 0..loops.max(1) {
        servo_pulse(pin, pulse);
        arduino_hal::delay_us(SERVO_PERIOD_US - pulse);
    }
}

// NEC IR command reader
fn read_nec_command(ir_pin: &Pin<Input<PullUp>, Dynamic>) -> Option<u8> {
    fn wait_for_level(
        pin: &Pin<Input<PullUp>, Dynamic>,
        level: bool,
        timeout_us: u32,
    ) -> Option<u32> {
        let mut t = 0;
        while !(if level { pin.is_high() } else { pin.is_low() }) {
            arduino_hal::delay_us(10);
            t += 10;
            if t > timeout_us {
                return None;
            }
        }
        Some(t)
    }

    wait_for_level(ir_pin, false, 100_000)?;

    // leading mark
    let mut lead_mark = 0;
    while ir_pin.is_low() {
        arduino_hal::delay_us(50);
        lead_mark += 50;
        if lead_mark > 12_000 {
            break;
        }
    }
    if !(7_000..=11_000).contains(&lead_mark) {
        return None;
    }

    // leading space
    let mut lead_space = 0;
    while ir_pin.is_high() {
        arduino_hal::delay_us(50);
        lead_space += 50;
        if lead_space > 6_000 {
            break;
        }
    }
    if !(3_500..=5_500).contains(&lead_space) {
        return None;
    }

    // read bits
    let mut bits: u32 = 0;
    for i in 0..32 {
        let mut mark = 0;
        while ir_pin.is_low() {
            arduino_hal::delay_us(50);
            mark += 50;
            if mark > 1200 {
                break;
            }
        }
        if !(200..=1200).contains(&mark) {
            return None;
        }

        let mut space = 0;
        while ir_pin.is_high() {
            arduino_hal::delay_us(50);
            space += 50;
            if space > 3000 {
                break;
            }
        }

        let bit = if space > 1000 { 1 } else { 0 };
        bits |= (bit as u32) << i;
    }

    let command = ((bits >> 16) & 0xFF) as u8;
    Some(command)
}

// Motion commands
fn left_move(times: u32, d10: &mut Pin<Output, Dynamic>, cs: &mut Serial) {
    for _ in 0..times {
        let angle = YAW_STOP.saturating_add(YAW_MOVE_SPEED as u8);
        servo_write_blocking(d10, angle, YAW_PRECISION_MS);
        servo_write_blocking(d10, YAW_STOP, 5);
        let _ = uwrite!(cs, "LEFT\r\n");
    }
}

fn right_move(times: u32, d10: &mut Pin<Output, Dynamic>, cs: &mut Serial) {
    for _ in 0..times {
        let angle = YAW_STOP.saturating_sub(YAW_MOVE_SPEED as u8);
        servo_write_blocking(d10, angle, YAW_PRECISION_MS);
        servo_write_blocking(d10, YAW_STOP, 5);
        let _ = uwrite!(cs, "RIGHT\r\n");
    }
}

fn up_move(times: u32, d11: &mut Pin<Output, Dynamic>, cs: &mut Serial) {
    for _ in 0..times {
        unsafe {
            if PITCH_VAL > PITCH_MIN {
                PITCH_VAL -= PITCH_MOVE_SPEED;
                let val = PITCH_VAL.clamp(0, 180) as u8;
                servo_write_blocking(d11, val, 100);
            }
        }
    }
    let _ = uwrite!(cs, "UP\r\n");
}

fn down_move(times: u32, d11: &mut Pin<Output, Dynamic>, cs: &mut Serial) {
    for _ in 0..times {
        unsafe {
            if PITCH_VAL < PITCH_MAX {
                PITCH_VAL += PITCH_MOVE_SPEED;
                let val = PITCH_VAL.clamp(0, 180) as u8;
                servo_write_blocking(d11, val, 100);
            }
        }
    }
    let _ = uwriteln!(cs, "DOWN");
}

fn fire(d12: &mut Pin<Output, Dynamic>, cs: &mut Serial) {
    let angle = ROLL_STOP.saturating_add(ROLL_MOVE_SPEED as u8);
    servo_write_blocking(d12, angle, ROLL_PRECISION_MS);
    servo_write_blocking(d12, ROLL_STOP, 5);
    let _ = uwriteln!(cs, "FIRING");
}

fn fire_all(d12: &mut Pin<Output, Dynamic>, cs: &mut Serial) {
    let angle = ROLL_STOP.saturating_add(ROLL_MOVE_SPEED as u8);
    servo_write_blocking(d12, angle, ROLL_PRECISION_MS * 6);
    servo_write_blocking(d12, ROLL_STOP, 5);
    let _ = uwriteln!(cs, "FIRING ALL");
}

fn home_servos(
    d10: &mut Pin<Output, Dynamic>,
    d11: &mut Pin<Output, Dynamic>,
    d12: &mut Pin<Output, Dynamic>,
    cs: &mut Serial,
) {
    servo_write_blocking(d10, YAW_STOP, 20);
    delay_ms(20);
    servo_write_blocking(d12, ROLL_STOP, 100);
    delay_ms(100);
    servo_write_blocking(d11, 100, 100);
    unsafe {
        PITCH_VAL = 100;
    }
    let _ = uwriteln!(cs, "HOMING");
}

// Serial command processor
fn process_serial_line(
    line: &str,
    d10: &mut Pin<Output, Dynamic>,
    d11: &mut Pin<Output, Dynamic>,
    d12: &mut Pin<Output, Dynamic>,
    cs: &mut Serial,
) {
    let s = line.trim();
    if s.is_empty() {
        return;
    }

    if s.eq_ignore_ascii_case("FIRE") {
        fire(d12, cs);
        let _ = uwriteln!(cs, "FIRE executed\r\n");
        return;
    }

    if s.len() < 2 {
        return;
    }

    let first = s.as_bytes()[0] as char;
    let rest = &s[1..];

    if let Ok(value) = rest.parse::<i32>() {
        match first {
            'H' | 'h' => {
                if value > 0 {
                    left_move(value as u32, d10, cs);
                } else if value < 0 {
                    right_move((-value) as u32, d10, cs);
                }
                let _ = uwriteln!(cs, "YAW moved by {}", value);
            }
            'V' | 'v' => {
                if value > 0 {
                    up_move(value as u32, d11, cs);
                } else if value < 0 {
                    down_move((-value) as u32, d11, cs);
                }
                let _ = uwriteln!(cs, "PITCH moved by {}", value);
            }
            _ => {}
        }
    }
}

//Entry point
#[arduino_hal::entry]
fn main() -> ! {
    let dp = Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);

    let mut serial = arduino_hal::default_serial!(dp, pins, 9600);
    let _ = uwriteln!(serial, "START nerf_rust_nano");

    // let ir_pin = pins.d9.into_pull_up_input().downgrade();
    let mut d10 = pins.d10.into_output().downgrade();
    let mut d11 = pins.d11.into_output().downgrade();
    let mut d12 = pins.d12.into_output().downgrade();

    home_servos(&mut d10, &mut d11, &mut d12, &mut serial);

    let mut buf: heapless::String<64> = heapless::String::new();

    let mut string_complete = false;
    serial.flush();

    loop {
        // Blocking read
        let byte = nb::block!(serial.read()).unwrap();
        // Minimal debug: Only log on newline to reduce UART interference
        if byte == b'\n' {
            ufmt::uwriteln!(&mut serial, "Line received! Buffer len: {}\r", buf.len()).unwrap();
            string_complete = true;
        }
        if buf.len() < buf.capacity() {
            buf.push(byte as char).ok(); // Ignore errors for simplicity
        }
        if string_complete && !buf.is_empty() {
            ufmt::uwriteln!(
                &mut serial,
                "RAW inputString: {} (len: {})\r",
                buf.as_str(),
                buf.len()
            )
            .unwrap();
            process_serial_line(&buf, &mut d10, &mut d11, &mut d12, &mut serial);
            buf.clear();
            ufmt::uwriteln!(&mut serial, "> ").unwrap();
            string_complete = false;
        }

        // Check IR remote (non-blocking)
        // if let Some(cmd) = read_nec_command(&ir_pin) {
        //     ufmt::uwriteln!(&mut serial, "IR cmd: {:#X}\r", cmd).unwrap();
        //     match cmd {
        //         CMD_UP => up_move(1, &mut d11, &mut serial),
        //         CMD_DOWN => down_move(1, &mut d11, &mut serial),
        //         CMD_LEFT => left_move(1, &mut d10, &mut serial),
        //         CMD_RIGHT => right_move(1, &mut d10, &mut serial),
        //         CMD_OK => fire(&mut d12, &mut serial),
        //         CMD_STAR => fire_all(&mut d12, &mut serial),
        //         _ => ufmt::uwriteln!(&mut serial, "UNKNOWN IR cmd: {:#X}\r", cmd).unwrap(),
        //     }
        // }
        // No delay to prevent UART buffer overflow
    }
}
