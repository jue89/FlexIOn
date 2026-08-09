#![cfg_attr(not(test), no_std)]
#![warn(unused_extern_crates)]

use embassy_time::{Duration, Ticker};
use log::{debug, warn};
use physical_values::{Ratio, Temperature};

pub use crate::{ntc::adc_to_temp, pid::Pid};

mod ntc;
mod pid;

pub struct CoolingController<AdcGetter: AsyncFnMut() -> u16, FanSpeedSetter: FnMut(Ratio)> {
    pub setpoint: Temperature,
    /// Triggers ADC sampling and returns a 16bit reading
    pub get_adc: AdcGetter,
    /// Changes the fan speed
    pub set_speed: FanSpeedSetter,
}

impl<AdcGetter: AsyncFnMut() -> u16, FanSpeedSetter: FnMut(Ratio)>
    CoolingController<AdcGetter, FanSpeedSetter>
{
    pub async fn run(self) -> ! {
        let Self {
            setpoint,
            mut get_adc,
            mut set_speed,
        } = self;

        let mut pid = Pid::new()
            .with_k_p(-200) // ‰/100 per dK
            .with_k_i(-10)
            .with_range(0..1000)
            .with_setpoint(setpoint.as_decimal(1));

        let mut ticker = Ticker::every(Duration::from_secs(3));

        loop {
            // Read NTC value
            let adc = get_adc().await;

            let pwm = if let Some(cur_temp) = adc_to_temp(adc) {
                // Run PID controller
                let pwm = Ratio::from_permill(pid.step(cur_temp.as_decimal(1)) as u16);
                debug!("Temperature: {}, PWM: {}", cur_temp, pwm);
                pwm
            } else {
                warn!("Cannot read NTC temperature!");

                // Fallback to full speed
                Ratio::from_percent(50)
            };

            // Update FAN speed
            set_speed(pwm);

            // Wait for next measurement
            ticker.next().await;
        }
    }
}
