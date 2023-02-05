#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::io::Error;
use std::{
    fs::File,
    io::{prelude::*, BufReader, ErrorKind},
};

#[cfg(feature = "hwmon")]
use libmedium::{
    parse_hwmons,
    sensors::{temp::TempSensor, Sensor}
};

#[cfg(feature = "cpuinfo")]
pub fn get_cpufreq() -> Result<f64, Error> {
    let file = File::open("/proc/cpuinfo")?;
    let mut file = BufReader::with_capacity(1024, file);

    let mut freqs: Vec<f64> = vec![];

    let mut line = String::with_capacity(256);
    while file.read_line(&mut line)? != 0 {
        let length = line.len();
        if length > 7 && length < 48 && &line[..7] == "cpu MHz" {
            match line[11..length - 1].parse::<f64>() {
                Ok(val) => freqs.push(val),
                Err(_) => {
                    line.clear();
                    continue;
                }
            };
        }
        line.clear();
    }

    match freqs.into_iter().reduce(f64::max) {
        Some(v) => Ok(v),
        None => Err(Error::new(ErrorKind::Other, "Couldn't get the cpufreq"))
    }
}

#[cfg(feature = "hwmon")]
pub fn get_hwmon_temp(hwmon_name: &str, sensor_name: &str) -> f64 {
    let hwmons = parse_hwmons().expect("couldn't parse hwmons!");

    let sensor = hwmons.iter().find(|hwmon| {
        hwmon.name() == hwmon_name
    }).expect("hwmon not found!").temps().iter().find(|(_, sensor)| {
        sensor.name() == sensor_name
    }).expect("sensor not found!").1.read_input().expect("couldn't read temp sensor!");

    sensor.as_degrees_celsius()
}
