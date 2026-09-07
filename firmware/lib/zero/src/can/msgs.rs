use buffer_tool::snip_ref;
use log::trace;
use phf::{Map, phf_map};
use physical_values::{Current, Ratio, Time, Voltage};

use crate::{
    can::{
        Msg,
        types::{
            AmpereHourU16, AmpereI16, DegCelsiusI8, MilliAmpereU16, MilliVoltU16, MilliVoltU32,
            Per255U8, PercentU8,
        },
    },
    state::{Mode, TemperatureRange, ZeroState},
};

pub static HANDLERS: Map<u32, fn(&[u8], &ZeroState)> = phf_map! {
    0x240 => handle_msg::<Dash>,
    0x288 => handle_msg::<PackConfig>,
    0x306 => handle_msg::<MaxChargeVoltageCurrent>,
    0x381 => handle_msg::<MotorController>,
    0x388 => handle_msg::<CellVoltage>,
    0x408 => handle_msg::<PackActiveData>,
};

fn handle_msg<M: Msg>(msg: &[u8], state: &ZeroState) {
    if let Some((msg, _)) = snip_ref::<M>(msg) {
        trace!("{:?}", msg);
        msg.handle(state);
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct CellVoltage {
    cell_idx: u8,
    cell_voltage: MilliVoltU16,
    pack_voltage: MilliVoltU32,
    _reserved: u8,
}

impl Msg for CellVoltage {
    fn handle(&self, state: &ZeroState) {
        state.pack_cells_acc.lock(|voltages| {
            let mut voltages = voltages.borrow_mut();
            if let Some(stats) = voltages.update(self.cell_idx as usize, self.cell_voltage.into()) {
                state.pack_cell_voltages.update(voltages.clone());
                state.pack_cell_stats.update(stats);
            }
        });
        state.pack_voltage.update(self.pack_voltage.into());
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct PackActiveData {
    _reserved: u8,
    temp_high: DegCelsiusI8,
    temp_low: DegCelsiusI8,
    discharge_current: AmpereI16,
    remaining_capacity: AmpereHourU16,
    allowed_current: Per255U8,
}

impl Msg for PackActiveData {
    fn handle(&self, state: &ZeroState) {
        state.pack_temp.update(TemperatureRange {
            low: self.temp_low.into(),
            high: self.temp_high.into(),
        });
        state
            .pack_discharge_current
            .update(self.discharge_current.into());
        state
            .pack_remaining_capacity
            .update(self.remaining_capacity.into());
        state
            .pack_allowed_current
            .update(self.allowed_current.into());
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct MaxChargeVoltageCurrent {
    _node_id: u8,
    max_charge_voltage: MilliVoltU32,
    max_charge_current: MilliAmpereU16,
    _reserved: u8,
}

impl Msg for MaxChargeVoltageCurrent {
    fn handle(&self, state: &ZeroState) {
        let max_charge_voltage = Voltage::from(self.max_charge_voltage);

        let mut max_charge_current = Current::from(self.max_charge_current);

        // Account for maximum representable charge current and
        // calculate it using the pack's capacity
        if max_charge_current == Current::from_millis(u16::MAX as i32)
            && let Some(pack_capacity) = state.pack_capacity.get()
            && let Some(allowed_current) = state.pack_allowed_current.get()
        {
            const HOUR: Time = Time::from_val(60 * 60);
            max_charge_current = (pack_capacity / HOUR) * allowed_current;
        }

        // Consider SoC
        if let Some(cur_soc) = state.pack_soc.get() {
            if cur_soc > Ratio::from_percent(91) {
                // Slow down charging at high SoC
                let derate = (cur_soc.as_percent() - 91) * 10;
                let ratio = Ratio::from_percent(100 - derate);
                max_charge_current *= ratio;
            } else if cur_soc < Ratio::from_percent(10) {
                // Slow down charging at low SoC
                let ratio = Ratio::from_percent((cur_soc.as_percent() + 1) * 10);
                max_charge_current *= ratio;
            };
        }

        state
            .pack_charge_limits
            .update((max_charge_voltage, max_charge_current));
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct PackConfig {
    _sag_adjustment: u16,
    _min_discharge_temp: DegCelsiusI8,
    min_charge_temp: DegCelsiusI8,
    max_charge_temp: DegCelsiusI8,
    pack_capacity: AmpereHourU16,
    _model_year: u8,
}

impl Msg for PackConfig {
    fn handle(&self, state: &ZeroState) {
        state.pack_charge_temp_limits.update(TemperatureRange {
            low: self.min_charge_temp.into(),
            high: self.max_charge_temp.into(),
        });
        state.pack_capacity.update(self.pack_capacity.into());
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct MotorController {
    _bat_voltage: u16,
    heatsink_temp: DegCelsiusI8,
    _bat_current: i16,
    _cap_voltage: u16,
    _di: u8,
}

impl Msg for MotorController {
    fn handle(&self, state: &ZeroState) {
        state.controller_temp.update(self.heatsink_temp.into());
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct Dash {
    mode: u8,
    _unkown: u8,
    _speed: u16,
    _power: u8,
    _torque: u8,
    soc: PercentU8,
}

impl Msg for Dash {
    fn handle(&self, state: &ZeroState) {
        if let Ok(mode) = Mode::try_from(self.mode) {
            state.bike_mode.update(mode);
        }
        state.pack_soc.update(self.soc.into());
    }
}
