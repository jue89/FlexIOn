#![cfg_attr(not(test), no_std)]
#![warn(unused_extern_crates)]

use embassy_futures::select::select;
use embassy_time::{Duration, Timer, WithTimeout as _};
use heapless::Vec;
use log::{debug, info, trace};
use physical_values::{Current, Frequency, Ratio};

const EVSE_FREQ: Frequency = Frequency::from_val(1000);
const EVER_FREQ_TOLERANCE: Ratio = Ratio::from_percent(1);

pub struct Evse<
    MainsCurrentLimitSetter: FnMut(Current),
    DisconnectRequest: AsyncFnMut(),
    EvsePwmGetter: AsyncFnMut() -> Option<(Frequency, Ratio)>,
    EvseRdySetter: FnMut(bool),
    DefaultCurrentLimitGetter: Fn() -> Current,
> {
    pub set_mains_current_limit: MainsCurrentLimitSetter,
    pub disconnect_request: DisconnectRequest,
    pub get_pwm: EvsePwmGetter,
    pub set_rdy: EvseRdySetter,
    pub get_default_current: DefaultCurrentLimitGetter,
}

impl<
    MainsCurrentLimitSetter: FnMut(Current),
    DisconnectRequest: AsyncFnMut(),
    EvsePwmGetter: AsyncFnMut() -> Option<(Frequency, Ratio)>,
    EvseRdySetter: FnMut(bool),
    DefaultCurrentLimitGetter: Fn() -> Current,
>
    Evse<
        MainsCurrentLimitSetter,
        DisconnectRequest,
        EvsePwmGetter,
        EvseRdySetter,
        DefaultCurrentLimitGetter,
    >
{
    pub async fn run(self) -> ! {
        let Self {
            mut set_mains_current_limit,
            mut disconnect_request,
            mut get_pwm,
            mut set_rdy,
            get_default_current,
        } = self;

        let expected_freq = EVSE_FREQ.as_range(EVER_FREQ_TOLERANCE);
        let mut get_pwm = async move || {
            const SAMPLES: usize = 32;
            const TIMEOUT: Duration = Duration::from_millis(10);
            let mut samples = Vec::<_, SAMPLES>::new();
            for _ in 0..=samples.capacity() {
                if let Some((freq, duty)) = get_pwm().with_timeout(TIMEOUT).await.ok().flatten()
                    && expected_freq.contains(&freq)
                {
                    let _ = samples.push(duty.as_permill());
                }
            }
            if samples.is_empty() {
                None
            } else {
                let acc = samples.iter().fold(0, |acc, ratio| acc + *ratio as u32);
                Some(Ratio::from_permill((acc / samples.len() as u32) as _))
            }
        };

        loop {
            // Try to figure out the maximum current
            let max_current = if let Some(duty) = get_pwm().await {
                trace!("PWM duty cycle: {:?}", duty);
                let max_current = match duty.as_percent() {
                    8..10 => Current::from_val(6),
                    percent @ 10..85 => Current::from_millis(percent as i32 * 600),
                    percent @ 85..96 => Current::from_millis((percent - 64) as i32 * 2500),
                    96..97 => Current::from_val(80),
                    _ => Current::from_val(0),
                };

                // Assert ready signal if more than 0A may be drawn
                set_rdy(max_current > Current::ZERO);

                debug!("EVSE pwm max_current {}", max_current);
                max_current
            } else {
                set_rdy(false);

                let max_current = get_default_current();
                debug!("EVSE default max_current {}", max_current);
                max_current
            };

            // Reflect the maximum current to the charger pack
            set_mains_current_limit(max_current);

            let event = select(
                // According to EN 61851 the delay to a change request must be <5s
                Timer::after(Duration::from_secs(3)),
                // Stop waiting if a cable disconnect has been requested!
                disconnect_request(),
            )
            .await;

            if event.is_second() {
                // Stop charging
                set_mains_current_limit(Current::ZERO);

                // We have been asked to disconenct from the EVSE,
                // so stop requesting power from the charging station
                set_rdy(false);

                info!("Released EVSE charging request, waiting for cable disconnect ...");

                loop {
                    Timer::after(Duration::from_secs(1)).await;

                    // Wait for the cable to be unplugged and, thus,
                    // the PWM signal to disappear
                    if let None = get_pwm().await {
                        info!("Cable disconnected!");

                        // Wait some time until reconnecting is possible
                        Timer::after(Duration::from_secs(30)).await;
                        break;
                    }
                }
            }
        }
    }
}
