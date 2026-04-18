#![no_std]
#![warn(unused_extern_crates)]

mod physical;
mod ratio;
pub mod units;

use core::ops::{Div, Mul};

use crate::units::{Ampere, AmpereHour, DegCelsius, Hz, Second, Volt, Watt};

pub use crate::{physical::Physical, ratio::Ratio};

pub type Time = Physical<Second>;
pub type Frequency = Physical<Hz>;
pub type Voltage = Physical<Volt>;
pub type Current = Physical<Ampere>;
pub type Power = Physical<Watt>;
pub type Capacity = Physical<AmpereHour>;
pub type Temperature = Physical<DegCelsius>;

impl Mul<Voltage> for Current {
    type Output = Power;

    fn mul(self, rhs: Voltage) -> Self::Output {
        let micros = self.millis as i64 * rhs.millis as i64;
        let millis = (micros / 1000) as i32;
        Power::from_millis(millis)
    }
}

impl Mul<Current> for Voltage {
    type Output = Power;

    fn mul(self, rhs: Current) -> Self::Output {
        rhs * self
    }
}

impl Div<Voltage> for Power {
    type Output = Current;

    fn div(self, rhs: Voltage) -> Self::Output {
        let micros = self.millis as i64 * 1000;
        let millis = (micros / rhs.millis as i64) as i32;
        Current::from_millis(millis)
    }
}

impl Div<Current> for Power {
    type Output = Voltage;

    fn div(self, rhs: Current) -> Self::Output {
        let micros = self.millis as i64 * 1000;
        let millis = (micros / rhs.millis as i64) as i32;
        Voltage::from_millis(millis)
    }
}

impl Div<Time> for Capacity {
    type Output = Current;

    fn div(self, rhs: Time) -> Self::Output {
        let mh = rhs.millis / 3600;
        Current::from_val(self.millis / mh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_physical_values() {
        assert_eq!(Voltage::from_val(1).millis, 1000);
        assert_eq!(Voltage::from_millis(1).millis, 1);
        assert_eq!(Voltage::from_decimal(1, 1).millis, 100);
        assert_eq!(Voltage::from_decimal(1, 2).millis, 10);
        assert_eq!(Voltage::from_decimal(1, 3).millis, 1);
        assert_eq!(Voltage::from_decimal(10, 4).millis, 1);
    }

    #[test]
    fn get_decimal() {
        assert_eq!(Voltage::from_val(1).as_decimal(0), 1);
        assert_eq!(Voltage::from_val(1).as_decimal(1), 10);
        assert_eq!(Voltage::from_val(1).as_decimal(2), 100);
        assert_eq!(Voltage::from_val(1).as_decimal(3), 1000);
        assert_eq!(Voltage::from_val(1).as_decimal(4), 10000);
    }

    #[test]
    fn get_smaller_value() {
        let u = Voltage::from_val(3).min(Voltage::from_val(2));
        assert_eq!(u, Voltage::from_val(2));
    }

    #[test]
    fn add_and_sub_physical_values() {
        let mut u = Voltage::from_val(230);
        u -= Voltage::from_val(30);
        assert_eq!(u, Voltage::from_val(200));

        let mut u = Voltage::from_val(230);
        u += Voltage::from_val(30);
        assert_eq!(u, Voltage::from_val(260));

        let u = Voltage::from_val(1) + Voltage::from_val(2);
        assert_eq!(u, Voltage::from_val(3));
    }

    #[test]
    fn calc_power() {
        let p = Voltage::from_val(230) * Current::from_val(16);
        assert_eq!(p, Power::from_val(230 * 16));

        let u = p / Current::from_val(16);
        assert_eq!(u, Voltage::from_val(230));

        let i = p / Voltage::from_val(230);
        assert_eq!(i, Current::from_val(16));
    }

    #[test]
    fn calc_current() {
        let c = Capacity::from_val(65);
        let t = Time::from_val(60 * 60);
        let i = c / t;
        assert_eq!(i, Current::from_val(65));
    }
}
