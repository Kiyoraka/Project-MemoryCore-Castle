//! A tiny stand-in "game" for the end-to-end test: three values on the heap whose addresses it
//! prints, changed and read back through stdin lines.
//!
//! stdin: `gold 1500` / `hp 99.5` / `time 12.25` set a value (echoes `gold 1500`);
//!        `read gold` echoes the current value. Ends when stdin closes.

use std::io::{BufRead, Write};

fn main() {
    let gold: Box<i32> = Box::new(1234);
    let hp: Box<f32> = Box::new(250.0);
    let time: Box<f64> = Box::new(30.0);
    let gold = Box::into_raw(gold);
    let hp = Box::into_raw(hp);
    let time = Box::into_raw(time);

    let mut out = std::io::stdout().lock();
    writeln!(out, "pid {}", std::process::id()).unwrap();
    writeln!(out, "gold 0x{:X}", gold as usize).unwrap();
    writeln!(out, "hp 0x{:X}", hp as usize).unwrap();
    writeln!(out, "time 0x{:X}", time as usize).unwrap();
    writeln!(out, "ready").unwrap();
    out.flush().unwrap();

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let parts: Vec<&str> = line.split_whitespace().collect();
        let reply = unsafe {
            match parts.as_slice() {
                ["read", "gold"] => format!("gold {}", std::ptr::read_volatile(gold)),
                ["read", "hp"] => format!("hp {}", std::ptr::read_volatile(hp)),
                ["read", "time"] => format!("time {}", std::ptr::read_volatile(time)),
                ["gold", v] => match v.parse::<i32>() {
                    Ok(v) => {
                        std::ptr::write_volatile(gold, v);
                        format!("gold {}", std::ptr::read_volatile(gold))
                    }
                    Err(_) => "error".into(),
                },
                ["hp", v] => match v.parse::<f32>() {
                    Ok(v) => {
                        std::ptr::write_volatile(hp, v);
                        format!("hp {}", std::ptr::read_volatile(hp))
                    }
                    Err(_) => "error".into(),
                },
                ["time", v] => match v.parse::<f64>() {
                    Ok(v) => {
                        std::ptr::write_volatile(time, v);
                        format!("time {}", std::ptr::read_volatile(time))
                    }
                    Err(_) => "error".into(),
                },
                _ => "error".into(),
            }
        };
        writeln!(out, "{reply}").unwrap();
        out.flush().unwrap();
    }
}
