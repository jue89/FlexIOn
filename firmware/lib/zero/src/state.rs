use core::{
    cell::RefCell,
    fmt::{Debug, Formatter, Result as FmtResult},
};

use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use physical_values::{Capacity, Current, Ratio, Temperature, Voltage};
use volatile_value::VolatileValue;

use crate::can::msgs::HANDLERS;

const MAX_OBSERVERS: usize = 2;
type ZeroVolatileValue<T, const UPDATE_INTERVAL_MS: u64 = 1000> =
    VolatileValue<T, MAX_OBSERVERS, UPDATE_INTERVAL_MS>;

const CELLS: usize = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Sport = 0b0000_0100,
    Eco = 0b0000_1000,
    Custom = 0b0001_0000,
}

impl TryFrom<u8> for Mode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let value = value & 0b0001_1100;
        if value == Self::Sport as u8 {
            Ok(Self::Sport)
        } else if value == Self::Eco as u8 {
            Ok(Self::Eco)
        } else if value == Self::Custom as u8 {
            Ok(Self::Custom)
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemperatureRange {
    pub low: Temperature,
    pub high: Temperature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellStats {
    pub min: Voltage,
    pub max: Voltage,
}

impl CellStats {
    fn new() -> Self {
        Self {
            min: Voltage::from_val(5),
            max: Voltage::ZERO,
        }
    }

    fn update(mut self, u: Voltage) -> Self {
        self.min = self.min.min(u);
        self.max = self.max.max(u);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellVoltages {
    pub cells: [Voltage; CELLS],
}

impl CellVoltages {
    pub(crate) const fn new() -> Self {
        Self {
            cells: [const { Voltage::ZERO }; CELLS],
        }
    }

    pub(crate) fn update(&mut self, idx: usize, u: Voltage) -> Option<CellStats> {
        let cell = self.cells.get_mut(idx)?;
        *cell = u;

        // Once the first cell is updated, calculate the balance
        if idx == 0 {
            let stats = self
                .cells
                .iter()
                .fold(CellStats::new(), |stats, u| stats.update(*u));
            if stats.min == Voltage::ZERO {
                // Not all cell voltages have been reported, yet
                None
            } else {
                Some(stats)
            }
        } else {
            None
        }
    }
}

pub struct ZeroState {
    /// Internal battery pack temperature
    pub pack_temp: ZeroVolatileValue<TemperatureRange>,

    /// Discharge current
    pub pack_discharge_current: ZeroVolatileValue<Current>,

    /// Remaining pack capacity
    pub pack_remaining_capacity: ZeroVolatileValue<Capacity>,

    /// The pack's capacity
    pub pack_capacity: ZeroVolatileValue<Capacity>,

    /// The pack's max current can be derived by multiplying this value with [Self::pack_capacity]
    pub pack_allowed_current: ZeroVolatileValue<Ratio>,

    /// Maximum voltage and current for charging
    ///
    /// The current can be derived by [Self::pack_capacity] * [Self::pack_allowed_current]
    pub pack_charge_limits: ZeroVolatileValue<(Voltage, Current)>,

    /// Temperatue range for charging the pack
    pub pack_charge_temp_limits: ZeroVolatileValue<TemperatureRange>,

    /// Cell voltages
    pub pack_cell_voltages: ZeroVolatileValue<CellVoltages, 10000>,

    /// Cell balance stats
    pub pack_cell_stats: ZeroVolatileValue<CellStats, 10000>,

    /// Pack voltage
    pub pack_voltage: ZeroVolatileValue<Voltage>,

    /// State of charge
    pub pack_soc: ZeroVolatileValue<Ratio>,

    /// Temperatur of the motor controller
    pub controller_temp: ZeroVolatileValue<Temperature>,

    /// Current drive mode
    pub bike_mode: ZeroVolatileValue<Mode>,

    /// Internal accumulator for cell voltages
    pub(crate) pack_cells_acc: Mutex<CriticalSectionRawMutex, RefCell<CellVoltages>>,
}

macro_rules! write_field {
    ($f:ident, $s:ident, $i:ident) => {
        write!($f, "\r\n\t{:<23} = {:?}", stringify!($i), $s.$i)
    };
}

impl Debug for ZeroState {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("ZeroState:")?;
        write_field!(f, self, bike_mode)?;
        write_field!(f, self, controller_temp)?;
        write_field!(f, self, pack_temp)?;
        write_field!(f, self, pack_discharge_current)?;
        write_field!(f, self, pack_capacity)?;
        write_field!(f, self, pack_remaining_capacity)?;
        write_field!(f, self, pack_soc)?;
        write_field!(f, self, pack_allowed_current)?;
        write_field!(f, self, pack_charge_limits)?;
        write_field!(f, self, pack_charge_temp_limits)?;
        write_field!(f, self, pack_voltage)?;
        write_field!(f, self, pack_cell_stats)?;
        Ok(())
    }
}

impl ZeroState {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            pack_temp: ZeroVolatileValue::new(),
            pack_discharge_current: ZeroVolatileValue::new(),
            pack_remaining_capacity: ZeroVolatileValue::new(),
            pack_capacity: ZeroVolatileValue::new(),
            pack_allowed_current: ZeroVolatileValue::new(),
            pack_charge_limits: ZeroVolatileValue::new(),
            pack_charge_temp_limits: ZeroVolatileValue::new(),
            pack_cell_voltages: ZeroVolatileValue::new(),
            pack_cell_stats: ZeroVolatileValue::new(),
            pack_voltage: ZeroVolatileValue::new(),
            pack_soc: ZeroVolatileValue::new(),
            controller_temp: ZeroVolatileValue::new(),
            bike_mode: ZeroVolatileValue::new(),
            pack_cells_acc: Mutex::new(RefCell::new(CellVoltages::new())),
        }
    }

    pub fn handle_can_msg(&self, id: u32, msg: &[u8]) {
        if let Some(handler) = HANDLERS.get(&id) {
            handler(msg, self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_config() {
        let state = ZeroState::new();
        state.handle_can_msg(0x288, &[0x31, 0x00, 0xE7, 0x00, 0x32, 0x39, 0x00, 0x13]);
        assert_eq!(
            state.pack_charge_temp_limits.get(),
            Some(TemperatureRange {
                low: Temperature::from_val(0),
                high: Temperature::from_val(50)
            })
        );
        assert_eq!(state.pack_capacity.get(), Some(Capacity::from_val(57)));
    }

    #[test]
    fn max_charge_voltage_current() {
        let state = ZeroState::new();
        state.handle_can_msg(0x306, &[0x13, 0xB0, 0xC6, 0x01, 0x00, 0x80, 0xBB, 0x00]);
        assert_eq!(
            state.pack_charge_limits.get(),
            Some((Voltage::from_millis(116400), Current::from_val(48)))
        );
    }

    #[test]
    fn max_charge_current_calculated() {
        let state = ZeroState::new();
        state.handle_can_msg(0x288, &[0x03, 0xFD, 0xE2, 0x00, 0x32, 0x68, 0x00, 0x10]); // Capacity: 104Ah
        state.handle_can_msg(0x408, &[0x00, 0x1A, 0x19, 0xEE, 0xFF, 0x38, 0x00, 0xCA]); // Max Current: 78.215%
        state.handle_can_msg(0x306, &[0x13, 0xB0, 0xC6, 0x01, 0x00, 0xFF, 0xFF, 0x00]); // Current: 65.353A
        assert_eq!(
            state.pack_charge_limits.get(),
            Some((Voltage::from_millis(116400), Current::from_millis(82368)))
        );
    }

    #[test]
    fn max_charge_current_derating() {
        let state = ZeroState::new();
        state.handle_can_msg(0x240, &[0x85, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00]); // SoC: 0%
        state.handle_can_msg(0x306, &[0x13, 0xB0, 0xC6, 0x01, 0x00, 0x80, 0xBB, 0x00]); // Current: 48A
        assert_eq!(
            state.pack_charge_limits.get().unwrap().1,
            Current::from_millis(4800)
        );

        state.handle_can_msg(0x240, &[0x85, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x0a]); // SoC: 10%
        state.handle_can_msg(0x306, &[0x13, 0xB0, 0xC6, 0x01, 0x00, 0x80, 0xBB, 0x00]); // Current: 48A
        assert_eq!(
            state.pack_charge_limits.get().unwrap().1,
            Current::from_millis(48000)
        );

        state.handle_can_msg(0x240, &[0x85, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x5C]); // SoC: 92%
        state.handle_can_msg(0x306, &[0x13, 0xB0, 0xC6, 0x01, 0x00, 0x80, 0xBB, 0x00]); // Current: 48A
        assert_eq!(
            state.pack_charge_limits.get().unwrap().1,
            Current::from_millis(43200)
        );

        state.handle_can_msg(0x240, &[0x85, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x60]); // SoC: 96%
        state.handle_can_msg(0x306, &[0x13, 0xB0, 0xC6, 0x01, 0x00, 0x80, 0xBB, 0x00]); // Current: 48A
        assert_eq!(
            state.pack_charge_limits.get().unwrap().1,
            Current::from_millis(24000)
        );

        state.handle_can_msg(0x240, &[0x85, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x64]); // SoC: 100%
        state.handle_can_msg(0x306, &[0x13, 0xB0, 0xC6, 0x01, 0x00, 0x80, 0xBB, 0x00]); // Current: 48A
        assert_eq!(
            state.pack_charge_limits.get().unwrap().1,
            Current::from_millis(4800)
        );
    }

    #[test]
    fn pack_active_data() {
        let state = ZeroState::new();
        state.handle_can_msg(0x408, &[0x00, 0x0F, 0x0F, 0xFB, 0xFF, 0x0F, 0x00, 0xDF]);
        assert_eq!(
            state.pack_temp.get(),
            Some(TemperatureRange {
                low: Temperature::from_val(15),
                high: Temperature::from_val(15)
            })
        );
        assert_eq!(
            state.pack_discharge_current.get(),
            Some(Current::from_val(-5))
        );
        assert_eq!(
            state.pack_remaining_capacity.get(),
            Some(Capacity::from_val(15))
        );
    }

    #[test]
    fn motor_controller() {
        let state = ZeroState::new();
        state.handle_can_msg(0x381, &[0x70, 0x06, 0x14, 0x00, 0x00, 0x7F, 0x06, 0x00]);
        assert_eq!(state.controller_temp.get(), Some(Temperature::from_val(20)));
    }

    #[test]
    fn dash() {
        let state = ZeroState::new();
        state.handle_can_msg(0x240, &[0x85, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x31]);
        assert_eq!(state.bike_mode.get(), Some(Mode::Sport));
        assert_eq!(state.pack_soc.get(), Some(Ratio::from_percent(49)));
    }
}
