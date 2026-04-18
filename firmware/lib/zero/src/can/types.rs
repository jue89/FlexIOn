use core::fmt::Debug;

use physical_values::{Capacity, Current, Ratio, Temperature, Voltage};

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct AmpereI16(i16);
impl From<AmpereI16> for Current {
    fn from(value: AmpereI16) -> Self {
        let u = i16::from_le(value.0) as i32;
        Self::from_val(u)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct MilliAmpereU16(u16);
impl From<MilliAmpereU16> for Current {
    fn from(value: MilliAmpereU16) -> Self {
        let u = u16::from_le(value.0) as i32;
        Self::from_millis(u)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct AmpereHourU16(u16);
impl From<AmpereHourU16> for Capacity {
    fn from(value: AmpereHourU16) -> Self {
        let u = u16::from_le(value.0) as i32;
        Self::from_val(u)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct MilliVoltU16(u16);
impl From<MilliVoltU16> for Voltage {
    fn from(value: MilliVoltU16) -> Self {
        let u = u16::from_le(value.0) as i32;
        Self::from_millis(u)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct MilliVoltU32(u32);
impl From<MilliVoltU32> for Voltage {
    fn from(value: MilliVoltU32) -> Self {
        let u = u32::from_le(value.0);
        let u = if u > i32::MAX as u32 {
            i32::MAX
        } else {
            u as i32
        };
        Self::from_millis(u)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct DegCelsiusI8(i8);
impl From<DegCelsiusI8> for Temperature {
    fn from(value: DegCelsiusI8) -> Self {
        Self::from_val(value.0 as i32)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct PercentU8(u8);
impl From<PercentU8> for Ratio {
    fn from(value: PercentU8) -> Self {
        Self::from_percent(value.0)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct Per255U8(u8);
impl From<Per255U8> for Ratio {
    fn from(value: Per255U8) -> Self {
        let ppm = value.0 as u32 * 3922;
        let permil = ppm / 1000;
        Self::from_permill(permil as u16)
    }
}
