#![cfg_attr(not(test), no_std)]
#![warn(unused_extern_crates)]

pub mod can;
mod charger;

use embassy_futures::select::{select, select_array};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    watch::{Receiver, Watch},
};
use heapless::Vec;
use log::info;
use physical_values::{Current, Voltage};

use crate::{
    can::{RxMsg, TxMsg},
    charger::TcCharger,
};

pub struct TcChargerPack<const N: usize> {
    chargers: [TcCharger; N],
    can_tx: Channel<CriticalSectionRawMutex, TxMsg, N>,
    online_cnt: Watch<CriticalSectionRawMutex, usize, 1>,
}

impl TcChargerPack<3> {
    pub const fn default_pack() -> Self {
        Self::new([
            TcCharger::new("CAN997", 0x18FF50E8, 0x1806E8F4),
            TcCharger::new("CAN998", 0x18FF50E7, 0x1806E7F4),
            TcCharger::new("CAN1430", 0x18FF50E5, 0x1806E5F4),
        ])
    }
}

impl<const N: usize> TcChargerPack<N> {
    pub const fn new(chargers: [TcCharger; N]) -> Self {
        let can_tx = Channel::new();
        let online_cnt = Watch::new();
        Self {
            chargers,
            can_tx,
            online_cnt,
        }
    }

    /// This must be called only once!
    pub fn get_online_cnt_observer<'a>(
        &'a self,
    ) -> Receiver<'a, CriticalSectionRawMutex, usize, 1> {
        self.online_cnt.receiver().unwrap()
    }

    /// Run this in an async task befor interacting with this module
    pub fn task(&self, get_pack_voltage: impl Fn() -> Option<Voltage> + Copy) -> impl Future {
        let futs = self.chargers.each_ref().map(|c| {
            let online_cnt = self.online_cnt.dyn_sender();
            let can_tx = self.can_tx.dyn_sender();
            select(c.online_tracking_task(online_cnt), async move {
                let offset = c.calibrate(get_pack_voltage).await;
                info!("TcCharger {} calibrated to offset={}", c.name, offset);
                c.ctrl_task(can_tx, offset).await;
            })
        });
        select_array(futs)
    }

    /// Get the next CAN message to be sent to the chargers
    pub fn next_can_tx_msg(&self) -> impl Future<Output = TxMsg> {
        self.can_tx.receive()
    }

    /// Handle an incoming CAN message
    pub fn handle_can_msg(&self, msg: RxMsg) {
        for c in self.chargers.iter() {
            if c.handle_can_msg(msg).is_break() {
                break;
            }
        }
    }

    /// Updates the limits for the mains side of the chargers.
    /// Make sure to update this value at least every 4 seconds!
    pub fn update_mains_limits_per_charger(&self, amps: Current) {
        for charger in self.chargers.iter() {
            charger.update_limits_in(amps);
        }
    }

    /// Update the limits for the secondary side of the chargers.
    /// Make sure to update this value at least every 4 seconds!
    pub fn update_out_limits(&self, volts: Voltage, amps: Current) {
        // Count how many chargers are active
        let online_chargers = self.online_cnt.try_get().unwrap_or(0);

        // Calc setpoint current
        let amps_per_charger = if online_chargers > 0 {
            amps / online_chargers
        } else {
            Current::ZERO
        };

        let mut chargers_settled = true;
        let mut increase = Vec::<&TcCharger, N>::new();
        for c in self.chargers.iter() {
            if !c.is_online() {
                // If the charger isn't active make sure it won't start
                // charging when it comes back.
                c.update_limits_out((Voltage::ZERO, Current::ZERO));
            } else if let Some((_, cur_max_amps)) = c.get_limits_out() {
                // Update the current out limits
                if cur_max_amps > amps_per_charger {
                    // The charger is asked to reduce output current
                    c.update_limits_out((volts, amps_per_charger));
                    info!("reduce {} to {} {}", c.name, volts, amps_per_charger);
                } else if cur_max_amps < amps_per_charger {
                    // Defere increasing currents to later, if all chargers
                    // setteled to their expected current
                    let _ = increase.push(c);
                } else {
                    // Make sure the voltage is up-to-date!
                    c.update_limits_out((volts, amps_per_charger));
                }

                // Look if the charger setted to it's setpoint
                chargers_settled &= c.out_current_settled();
            } else {
                // The charger don't have a setpoint, yet.
                // Put it into the increase list.
                let _ = increase.push(c);
            }
        }

        // Only increase currents if all other chargers settled to their setpoint.
        // There is a significant delay between the request to reduce current and
        // the acutal response. Waiting for all chargers to settle before increasing
        // currents ensures that the summed current doesn't exceed the overall limit.
        if chargers_settled {
            for c in increase {
                c.update_limits_out((volts, amps_per_charger));
                info!("increase {} to {} {}", c.name, volts, amps_per_charger);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use embassy_futures::{select::select, yield_now};
    use embassy_time::{Duration, MockDriver};
    use physical_values::{Current, Power, Voltage};

    use crate::{
        TcChargerPack,
        can::{RxMsg, RxPayload, TxMsg, TxPayload},
        charger::TcCharger,
    };

    fn mk_pack() -> TcChargerPack<3> {
        TcChargerPack::new([
            TcCharger::new("a", 0x00, 0x01),
            TcCharger::new("b", 0x10, 0x11),
            TcCharger::new("c", 0x20, 0x21),
        ])
    }

    async fn report<const N: usize>(
        pack: &TcChargerPack<N>,
        name: &'static str,
        u: Voltage,
        i: Current,
    ) {
        let charger = pack.chargers.iter().find(|c| c.name == name).unwrap();
        let payload = RxPayload {
            deci_volts_be: (u.as_decimal(1) as u16).to_be(),
            deci_amps_be: (i.as_decimal(1) as u16).to_be(),
            _status: 0,
            _temp: 0,
            _reserved: [0u8; _],
        };
        let msg = RxMsg {
            id: charger.can_id_rx,
            payload: &payload,
        };
        assert!(charger.handle_can_msg(msg).is_break());
        yield_now().await;
        MockDriver::get().advance(Duration::from_ticks(1));
    }

    async fn assert_can_tx<const N: usize>(
        pack: &TcChargerPack<N>,
        name: &'static str,
        u: Voltage,
        i: Current,
    ) {
        let charger = pack.chargers.iter().find(|c| c.name == name).unwrap();
        let TxMsg { id, payload } = pack.next_can_tx_msg().await;
        assert_eq!(id, charger.can_id_tx);
        assert_eq!(
            payload,
            TxPayload {
                deci_volts_be: (u.as_decimal(1) as u16).to_be(),
                deci_amps_be: (i.as_decimal(1) as u16).to_be(),
                control: if u * i > Power::ZERO { 0 } else { 1 },
                _reserved: [0u8; _],
            }
        );
    }

    #[embassy_unittest::test]
    async fn split_out_current() {
        let pack = mk_pack();
        let set_u = Voltage::from_val(116);
        select(pack.task(|| None), async {
            let report = async |name: &'static str, i: Current| {
                report(&pack, name, Voltage::from_val(100), i).await;
            };
            let report_and_assert =
                async |name: &'static str, i_report: Current, u_set: Voltage, i_set: Current| {
                    report(name, i_report).await;
                    assert_can_tx(&pack, name, u_set, i_set).await;
                };

            // Set in limits to high value
            pack.update_mains_limits_per_charger(Current::from_val(16));

            // Mark two chargers online and run calibration
            report("a", Current::ZERO).await;
            report("b", Current::ZERO).await;
            report_and_assert("a", Current::ZERO, Voltage::ZERO, Current::ZERO).await;
            report_and_assert("b", Current::ZERO, Voltage::ZERO, Current::ZERO).await;

            // Request 6A
            let set_i = Current::from_val(9);
            pack.update_out_limits(set_u, set_i);
            report_and_assert("a", Current::ZERO, set_u, set_i / 2).await;
            report_and_assert("b", Current::ZERO, set_u, set_i / 2).await;

            // Start and calibrate third charger
            report("c", Current::ZERO).await;
            report_and_assert("a", set_i / 2, set_u, set_i / 2).await;
            report_and_assert("b", set_i / 2, set_u, set_i / 2).await;
            report_and_assert("c", Current::ZERO, Voltage::ZERO, Current::ZERO).await;

            // Request 6A -> a + b have to be throttled before c is starting to ramp-up current
            pack.update_out_limits(set_u, set_i);
            report_and_assert("a", set_i / 2, set_u, set_i / 3).await;
            report_and_assert("b", set_i / 2, set_u, set_i / 3).await;
            report_and_assert("c", Current::ZERO, Voltage::ZERO, Current::ZERO).await;

            // ... and chargers settled
            report_and_assert("a", set_i / 3, set_u, set_i / 3).await;
            report_and_assert("b", set_i / 3, set_u, set_i / 3).await;
            report_and_assert("c", Current::ZERO, Voltage::ZERO, Current::ZERO).await;

            // ... and now the we can increase power of charger c
            pack.update_out_limits(set_u, set_i);
            report_and_assert("a", set_i / 3, set_u, set_i / 3).await;
            report_and_assert("b", set_i / 3, set_u, set_i / 3).await;
            report_and_assert("c", Current::ZERO, set_u, set_i / 3).await;
        })
        .await;
    }
}
