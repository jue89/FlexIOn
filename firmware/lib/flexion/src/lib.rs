#![no_std]

use embassy_futures::select::{Either4, select4};
use embassy_time::{Duration, Timer};
use log::{info, warn};
use physical_values::{Current, Power, Ratio, Voltage};

pub struct ChargeLimits {
    pub soc_limit: Option<Ratio>,
    pub power_limit: Option<Power>,
}

pub struct ChargeCtrl<
    UserLimitsGetter: Fn() -> ChargeLimits,
    PackLimitsGetter: AsyncFnMut() -> Option<(Voltage, Current)>,
    PackSocGetter: AsyncFnMut() -> Option<Ratio>,
    ChargerCntObserver: AsyncFnMut() -> usize,
    ChargerAttachSetter: FnMut(bool),
    ChargerEnableObserver: AsyncFnMut(bool),
    ChargeLimitsSetter: FnMut(Voltage, Current),
> {
    pub get_user_limits: UserLimitsGetter,
    pub get_pack_limits: PackLimitsGetter,
    pub get_pack_soc: PackSocGetter,
    pub active_charger_cnt_change: ChargerCntObserver,
    pub set_charger_attach: ChargerAttachSetter,
    pub charger_enable_level: ChargerEnableObserver,
    pub set_charge_limits: ChargeLimitsSetter,
}

impl<
    UserLimitsGetter: Fn() -> ChargeLimits,
    PackLimitsGetter: AsyncFnMut() -> Option<(Voltage, Current)>,
    PackSocGetter: AsyncFnMut() -> Option<Ratio>,
    ChargerCntObserver: AsyncFnMut() -> usize,
    ChargerAttachSetter: FnMut(bool),
    ChargerEnableObserver: AsyncFnMut(bool),
    ChargeLimitsSetter: FnMut(Voltage, Current),
>
    ChargeCtrl<
        UserLimitsGetter,
        PackLimitsGetter,
        PackSocGetter,
        ChargerCntObserver,
        ChargerAttachSetter,
        ChargerEnableObserver,
        ChargeLimitsSetter,
    >
{
    pub async fn run(self) -> ! {
        let Self {
            get_user_limits,
            mut get_pack_limits,
            mut get_pack_soc,
            mut active_charger_cnt_change,
            mut set_charger_attach,
            mut charger_enable_level,
            mut set_charge_limits,
        } = self;

        loop {
            info!("⏳ Waiting for chargers to become active ...");

            // Wait for active chargers
            let chargers = loop {
                let cnt = active_charger_cnt_change().await;
                if cnt > 0 {
                    break cnt;
                }
            };

            // Let the bike mode decide the max soc
            let ChargeLimits {
                soc_limit,
                power_limit,
            } = get_user_limits();
            info!(
                "🔌 Charging request: soc_limit={:?}, power_limit={:?}, charger_cnt={:?}",
                soc_limit, power_limit, chargers
            );

            // Assert charger attach signal
            set_charger_attach(true);

            // Wait for charger enable signal
            charger_enable_level(true).await;

            info!("🔋 Request granted by bike. Start charging ...");

            // Get max. charge current and voltage
            let delay = loop {
                match select4(
                    get_pack_limits(),
                    get_pack_soc(),
                    charger_enable_level(false),
                    active_charger_cnt_change(),
                )
                .await
                {
                    // We got fresh info from the bike about max charging current
                    Either4::First(Some((max_u, max_i))) => {
                        // Consider configured power limit
                        let max_i = match power_limit {
                            Some(max_p) => max_p.min(max_u * max_i) / max_u,
                            None => max_i,
                        };
                        set_charge_limits(max_u, max_i);
                    }
                    // We don't have any info about max charge current anymore!
                    Either4::First(None) => {
                        warn!("🤔 BMS isn't reporting charge limits anymore");
                        // Retry in 5min
                        break Duration::from_secs(60 * 5);
                    }
                    // We reached the SoC
                    Either4::Second(Some(soc))
                        if let Some(soc_limit) = soc_limit
                            && soc >= soc_limit =>
                    {
                        info!("🏁 Reached soc_limit of {}", soc);
                        // Retry in 7 days
                        break Duration::from_secs(7 * 24 * 60 * 60);
                    }
                    // Nop for all other SoC messages
                    Either4::Second(_) => {}
                    // The bike asked to stop charging
                    Either4::Third(()) => {
                        info!("🛑 Bike withdraw charge enable signal");
                        // Retry in 7 days
                        break Duration::from_secs(7 * 24 * 60 * 60);
                    }
                    // All chargers disappeared
                    Either4::Fourth(0) => {
                        info!("👋 Chargers went offline");
                        // Retry immediatly
                        break Duration::from_secs(10);
                    }
                    // The charger count just changed
                    Either4::Fourth(_) => {}
                }
            };

            // Shutdown charger
            set_charge_limits(Voltage::ZERO, Current::ZERO);

            // Shutdown charger attach signal
            set_charger_attach(false);

            info!(
                "✋ Stopped charging. Waiting {}min before restarting.",
                delay.as_secs() / 60
            );

            // Wait returned delay until restarting ...
            Timer::after(delay).await;
        }
    }
}
