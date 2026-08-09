use core::{
    ops::ControlFlow,
    sync::atomic::{AtomicBool, Ordering},
};

use embassy_sync::{channel::DynamicSender as ChannelSender, watch::DynSender as WatchSender};
use log::{debug, info};
use physical_values::{Current, Ratio, Voltage};
use volatile_value::VolatileValue;

use crate::can::{RxMsg, TxMsg, TxPayload, VoltageCurrent};

pub struct TcCharger {
    pub name: &'static str,
    pub can_id_rx: u32,
    pub can_id_tx: u32,
    current_settled: AtomicBool,
    out_actual: VolatileValue<VoltageCurrent, 2, 2000>,
    out_limits: VolatileValue<VoltageCurrent, 0, 5000>,
    in_limits: VolatileValue<Current, 0, 5000>,
}

impl TcCharger {
    const MAINS_VOLTAGE_NOMINAL: Voltage = Voltage::from_val(230);
    const MAINS_VOLTAGE_RATIO: Ratio = Ratio::from_percent(95);
    const CHARGER_EFFICIENCY: Ratio = Ratio::from_percent(95); // according to the datasheet
    const MIN_BAT_VOLTAGE: Voltage = Voltage::from_val(80);

    pub const fn new(name: &'static str, can_id_rx: u32, can_id_tx: u32) -> Self {
        Self {
            name,
            can_id_rx,
            can_id_tx,
            current_settled: AtomicBool::new(true),
            out_actual: VolatileValue::new(),
            out_limits: VolatileValue::new(),
            in_limits: VolatileValue::new(),
        }
    }

    pub fn handle_can_msg(&self, msg: RxMsg) -> ControlFlow<()> {
        if msg.id == self.can_id_rx {
            let actual = msg.payload.decode();
            debug!("TcCharger {} Actual  : {:?}", self.name, actual);
            self.out_actual.update(actual);
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    pub fn is_online(&self) -> bool {
        self.out_actual.get().is_some()
    }

    pub fn out_current_settled(&self) -> bool {
        self.current_settled.load(Ordering::Acquire)
    }

    pub fn get_actual_out(&self) -> Option<VoltageCurrent> {
        self.out_actual.get()
    }

    pub fn get_limits_out(&self) -> Option<VoltageCurrent> {
        self.out_limits.get()
    }

    pub fn update_limits_out(&self, (u, i): VoltageCurrent) {
        if let Some((_, old_i)) = self.out_limits.get()
            && old_i > i
        {
            // If current limit has lowered mark the charger as not settled
            self.current_settled.store(false, Ordering::Release);
        }
        self.out_limits.update((u, i));
    }

    pub fn update_limits_in(&self, i: Current) {
        self.in_limits.update(i);
    }

    pub async fn online_tracking_task<'a>(
        &'a self,
        charger_online_cnt: WatchSender<'a, usize>,
    ) -> ! {
        let mut out_actual_observer = self.out_actual.observer().unwrap();
        let mut is_online = false;
        loop {
            let out_actual = out_actual_observer.change().await;
            if !is_online && out_actual.is_some() {
                // Charger truned online
                is_online = true;
                charger_online_cnt.send_modify(|cnt| {
                    let new_cnt = cnt.unwrap_or(0) + 1;
                    *cnt = Some(new_cnt);
                });
                info!("TcCharger {} turned online", self.name);
            } else if is_online && out_actual.is_none() {
                // Charger turned offline
                is_online = false;
                charger_online_cnt.send_modify(|cnt| {
                    let new_cnt = cnt.unwrap_or(0).saturating_sub(1);
                    *cnt = Some(new_cnt);
                });
                info!("TcCharger {} turned offline", self.name);
            }
        }
    }

    pub async fn ctrl_task<'a>(&'a self, can_tx: ChannelSender<'a, TxMsg>) -> ! {
        let mut out_actual_observer = self.out_actual.observer().unwrap();

        loop {
            let out_actual = out_actual_observer.change().await;

            let (set_u, set_i, settled) = match out_actual {
                // Charger is online, connected to the battery and has fresh setpoints
                Some((act_out_u, act_out_i))
                    if act_out_u >= Self::MIN_BAT_VOLTAGE
                        && let Some((max_out_u, max_out_i)) = self.out_limits.get()
                        && let Some(max_in_i) = self.in_limits.get() =>
                {
                    // Take max current from input side into account
                    let max_in_p =
                        Self::MAINS_VOLTAGE_NOMINAL * Self::MAINS_VOLTAGE_RATIO * max_in_i;
                    let max_out_p = max_in_p * Self::CHARGER_EFFICIENCY;
                    let max_out_i = (max_out_p / act_out_u).min(max_out_i);

                    // Check if the charger already settled to that value
                    let settled = {
                        let diff = max_out_i - act_out_i;
                        const MAX_DIFF: Current = Current::from_val(1);
                        (-MAX_DIFF..=MAX_DIFF).contains(&diff)
                    };

                    (max_out_u, max_out_i, settled)
                }
                // Charger is not ready to charge
                _ => (Voltage::ZERO, Current::ZERO, false),
            };

            if settled {
                // Mark the charger settled again
                self.current_settled.store(true, Ordering::Relaxed);
            }

            // Craft CAN message
            can_tx
                .send(TxMsg {
                    id: self.can_id_tx,
                    payload: TxPayload::new((set_u, set_i)),
                })
                .await;

            debug!("TcCharger {} Setpoint: {:?}", self.name, (set_u, set_i));
        }
    }
}

#[cfg(test)]
mod tests {
    use embassy_futures::{select::select, yield_now};
    use embassy_sync::{
        blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, watch::Watch,
    };
    use embassy_time::{Duration, MockDriver};
    use physical_values::{Current, Power, Ratio, Voltage};

    use crate::{
        can::{RxMsg, RxPayload, TxMsg, TxPayload},
        charger::TcCharger,
    };

    async fn report(charger: &TcCharger, u: Voltage, i: Current) {
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

    type CanTxCh = Channel<CriticalSectionRawMutex, TxMsg, 8>;
    type OnlineCnt = Watch<CriticalSectionRawMutex, usize, 1>;

    async fn assert_can_tx(charger: &TcCharger, ch: &CanTxCh, u: Voltage, i: Current) {
        let TxMsg { id, payload } = ch.receive().await;
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
    async fn limit_in_current() {
        let charger = TcCharger::new("foo", 0x1, 0x2);

        let in_current = Current::from_val(1);
        let in_voltage = Voltage::from_val(230) * Ratio::from_percent(95);
        let out_voltage = Voltage::from_val(100);
        let out_power = in_current * in_voltage * Ratio::from_percent(95);
        let out_current = out_power / out_voltage;
        let out_setpoint = Voltage::from_val(116);

        let can_tx = CanTxCh::new();

        select(charger.ctrl_task(can_tx.dyn_sender()), async {
            charger.update_limits_in(in_current);
            charger.update_limits_out((out_setpoint, Current::from_val(32)));
            report(&charger, out_voltage, Current::ZERO).await;
            assert_can_tx(&charger, &can_tx, out_setpoint, out_current).await;
        })
        .await;
    }

    #[embassy_unittest::test]
    async fn update_online_cnt() {
        let charger = TcCharger::new("foo", 0x1, 0x2);

        let online_cnt = OnlineCnt::new();

        select(
            charger.online_tracking_task(online_cnt.dyn_sender()),
            async {
                let mut online_cnt = online_cnt.receiver().unwrap();

                // Wait for online
                report(&charger, Voltage::ZERO, Current::ZERO).await;
                assert_eq!(online_cnt.changed().await, 1);

                // ... and offline again
                MockDriver::get().advance(Duration::from_secs(2));
                assert_eq!(online_cnt.changed().await, 0);
            },
        )
        .await;
    }

    #[embassy_unittest::test]
    async fn settle_charger() {
        let charger = TcCharger::new("foo", 0x1, 0x2);
        let can_tx = CanTxCh::new();

        let current = Current::from_val(5);
        charger.update_limits_in(Current::from_val(16));

        // Increas and decrase limits to mark the charger non-setteled
        charger.update_limits_out((Voltage::from_val(100), current + Current::from_val(1)));
        charger.update_limits_out((Voltage::from_val(100), current));

        select(charger.ctrl_task(can_tx.dyn_sender()), async {
            let report = async |i: Current| {
                report(&charger, Voltage::from_val(99), i).await;
            };

            report(current - Current::from_val(2)).await;
            assert_eq!(charger.out_current_settled(), false);

            report(current - Current::from_val(1)).await;
            assert_eq!(charger.out_current_settled(), true);

            report(current).await;
            assert_eq!(charger.out_current_settled(), true);
        })
        .await;
    }
}
