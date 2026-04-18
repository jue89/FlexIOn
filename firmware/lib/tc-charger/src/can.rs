use core::mem::transmute;

use buffer_tool::snip_ref;
use physical_values::{Current, Power, Voltage};

pub const CAN_MSG_LEN: usize = 8;
pub type CanPayload = [u8; CAN_MSG_LEN];
pub type VoltageCurrent = (Voltage, Current);

#[derive(Debug, PartialEq)]
pub struct TxMsg {
    pub id: u32,
    pub payload: TxPayload,
}

#[derive(Clone, Copy, Debug)]
pub struct RxMsg<'a> {
    pub id: u32,
    pub payload: &'a RxPayload,
}

#[derive(Debug, PartialEq)]
#[repr(C, packed)]
pub struct TxPayload {
    pub(crate) deci_volts_be: u16,
    pub(crate) deci_amps_be: u16,
    pub(crate) control: u8,
    pub(crate) _reserved: [u8; 3],
}

impl TxPayload {
    pub fn new(val: VoltageCurrent) -> Self {
        let (u, i) = val;
        Self {
            deci_volts_be: (u.as_decimal(1) as u16).to_be(),
            deci_amps_be: (i.as_decimal(1) as u16).to_be(),
            control: if u * i == Power::ZERO { 1 } else { 0 },
            _reserved: [0u8; _],
        }
    }
}

impl Into<CanPayload> for TxPayload {
    fn into(self) -> CanPayload {
        unsafe { transmute(self) }
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct RxPayload {
    pub(crate) deci_volts_be: u16,
    pub(crate) deci_amps_be: u16,
    pub(crate) _status: u8,
    pub(crate) _temp: u8,
    pub(crate) _reserved: [u8; 2],
}

impl RxPayload {
    pub fn decode(&self) -> VoltageCurrent {
        let u = Voltage::from_decimal(u16::from_be(self.deci_volts_be) as i32, 1);
        let i = Current::from_decimal(u16::from_be(self.deci_amps_be) as i32, 1);
        (u, i)
    }
}

impl<'a> TryInto<&'a RxPayload> for &'a [u8] {
    type Error = ();

    fn try_into(self) -> Result<&'a RxPayload, Self::Error> {
        let (msg, _) = snip_ref::<RxPayload>(self).ok_or(())?;
        Ok(msg)
    }
}
