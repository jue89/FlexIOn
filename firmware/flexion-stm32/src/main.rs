#![no_std]
#![no_main]
#![warn(unused_extern_crates)]

use core::panic::PanicInfo;

#[cfg(feature = "defmt")]
use defmt_rtt as _;
use embassy_executor::{Spawner, main, task};
use embassy_stm32::{
    Config, bind_interrupts,
    exti::{ExtiInput, InterruptHandler},
    gpio::{Level, Output, OutputOpenDrain, Pull, Speed},
    init, interrupt,
    rcc::{ADCPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPreDiv, PllSource, Sysclk},
    time::Hertz,
};
use embassy_time::Timer;
use flexion::{self, ChargeCtrl, ChargeLimits};
use log::info;
use physical_values::{Current, Power, Ratio};
use tc_charger::TcChargerPack;
use zero::{Mode, ZeroState};

mod can_tc_charger;
mod can_zero;
mod debug;

bind_interrupts!(pub struct Irqs{
    EXTI15_10 => InterruptHandler<interrupt::typelevel::EXTI15_10>;
});

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    cortex_m::peripheral::SCB::sys_reset()
}

fn mk_rcc_config() -> Config {
    let mut config = Config::default();

    // Disable internal high speed clock
    config.rcc.hsi = false;

    // External crystal with 12MHz
    config.rcc.hse = Some(Hse {
        freq: Hertz::mhz(12),
        mode: HseMode::Oscillator,
    });

    // Use the PLL to bringt the clock up to 72MHz
    config.rcc.pll = Some(Pll {
        src: PllSource::HSE,
        prediv: PllPreDiv::DIV1,
        mul: PllMul::MUL6,
    });

    // Run the system on 72MHz
    config.rcc.sys = Sysclk::PLL1_P;

    // But reduce APB1 periph clock to 36MHz (this is max!)
    config.rcc.apb1_pre = APBPrescaler::DIV2;

    // Run the ADCs with 12MHz
    config.rcc.adc_pre = ADCPrescaler::DIV6;

    config
}

#[task]
async fn evse(charger: &'static TcChargerPack<3>) {
    loop {
        charger.update_mains_limits_per_charger(Current::from_val(2));
        Timer::after_secs(3).await;
    }
}

#[task]
async fn print_zero_stats(zero_state: &'static ZeroState) {
    loop {
        Timer::after_secs(10).await;
        info!("{:?}", zero_state);
    }
}

#[main]
async fn main(spawner: Spawner) {
    static ZERO_STATE: ZeroState = ZeroState::new();
    static CHARGER_PACK: TcChargerPack<3> = TcChargerPack::default_pack();

    let p = init(mk_rcc_config());

    let _led_a = Output::new(p.PB11, Level::High, Speed::Low);

    debug::init(spawner, p.USART1, p.PA9, p.PA10, p.DMA1_CH4, p.DMA1_CH5);

    Timer::after_millis(1).await;

    can_zero::init(spawner, &ZERO_STATE, p.CAN2, p.PB12, p.PB13).await;
    can_tc_charger::init(spawner, &CHARGER_PACK, &ZERO_STATE, p.CAN1, p.PB8, p.PB9).await;
    spawner.spawn(evse(&CHARGER_PACK).unwrap());
    spawner.spawn(print_zero_stats(&ZERO_STATE).unwrap());

    // Main control task
    ChargeCtrl {
        get_user_limits: || match ZERO_STATE.bike_mode.get() {
            // Bike is powered-on
            // -> Fast charging for bike trips
            Some(Mode::Sport) => ChargeLimits {
                soc_limit: None,
                power_limit: None,
            },
            // -> Charging full for longer rides on the next day
            Some(Mode::Custom) => ChargeLimits {
                soc_limit: None,
                power_limit: Some(Power::from_val(2200)),
            },
            // -> Charging for winter break
            Some(Mode::Eco) => ChargeLimits {
                soc_limit: Some(Ratio::from_percent(60)),
                power_limit: Some(Power::from_val(800)),
            },
            // Bike is sleeping
            // -> Charging for every-day commute
            None => ChargeLimits {
                soc_limit: Some(Ratio::from_percent(70)),
                power_limit: Some(Power::from_val(800)),
            },
        },
        get_pack_limits: {
            let mut observer = ZERO_STATE.pack_charge_limits.observer().unwrap();
            async move || observer.change().await
        },
        get_pack_soc: {
            let mut observer = ZERO_STATE.pack_soc.observer().unwrap();
            async move || observer.change().await
        },
        active_charger_cnt_change: {
            let mut observer = CHARGER_PACK.get_online_cnt_observer();
            async move || observer.changed().await
        },
        set_charger_attach: {
            let mut attach = OutputOpenDrain::new(p.PB14, Level::High, Speed::Low);
            move |en| {
                if en {
                    attach.set_low();
                } else {
                    attach.set_high();
                }
            }
        },
        charger_enable_level: {
            let mut enable = ExtiInput::new(p.PB15, p.EXTI15, Pull::None, Irqs);
            async move |en| {
                if en {
                    enable.wait_for_low().await;
                } else {
                    enable.wait_for_high().await;
                }
            }
        },
        set_charge_limits: |max_u, max_i| CHARGER_PACK.update_out_limits(max_u, max_i),
    }
    .run()
    .await;
}
