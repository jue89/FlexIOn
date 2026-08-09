#![no_std]
#![no_main]
#![warn(unused_extern_crates)]

use core::panic::PanicInfo;

use cooling::CoolingController;
#[cfg(feature = "defmt")]
use defmt_rtt as _;
use embassy_executor::{Spawner, main, task};
use embassy_stm32::{
    Config, Peri,
    adc::{self, Adc, SampleTime},
    bind_interrupts,
    exti::{ExtiInput, InterruptHandler},
    gpio::{AfioRemap, Level, Output, OutputOpenDrain, OutputType, Pull, Speed, SwjCfg},
    init, interrupt,
    peripherals::{
        self, ADC1, EXTI0, PA0, PA1, PA8, PA15, PB3, PB5, PB6, PC0, PC3, PC12, TIM1, TIM2, TIM4,
        TIM5,
    },
    rcc::{ADCPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPreDiv, PllSource, Sysclk},
    time::Hertz,
    timer::{
        CaptureCompareInterruptHandler, GeneralInstance4Channel,
        low_level::{CountingMode, OutputPolarity},
        pwm_input::PwmInput,
        simple_pwm::{PwmPin, SimplePwm, SimplePwmChannel},
    },
};
use embassy_time::Timer;
use evse::Evse;
use flexion::{self, ChargeCtrl, ChargeLimits};
use log::info;
use physical_values::{Current, Frequency, Power, Ratio, Temperature};
use tc_charger::TcChargerPack;
use zero::{Mode, ZeroState};

mod can_tc_charger;
mod can_zero;
mod debug;

bind_interrupts!(pub struct Irqs{
    EXTI0 => InterruptHandler<interrupt::typelevel::EXTI0>;
    EXTI15_10 => InterruptHandler<interrupt::typelevel::EXTI15_10>;
    TIM1_CC => CaptureCompareInterruptHandler<peripherals::TIM1>;
    TIM4 => CaptureCompareInterruptHandler<peripherals::TIM4>;
    TIM5 => CaptureCompareInterruptHandler<peripherals::TIM5>;
    ADC1_2 => adc::InterruptHandler<ADC1>;
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

    // Use the PLL to bringt the clock up to 36MHz
    config.rcc.pll = Some(Pll {
        src: PllSource::HSE,
        prediv: PllPreDiv::DIV2,
        mul: PllMul::MUL6,
    });

    // Run the system on 36MHz
    config.rcc.sys = Sysclk::PLL1_P;

    // But reduce APB1 periph clock to 36MHz (this is max!)
    config.rcc.apb1_pre = APBPrescaler::DIV1;

    // Run the ADCs with 6MHz
    config.rcc.adc_pre = ADCPrescaler::DIV6;

    // Make PB3 usable
    config.swj = SwjCfg::SwdOnly;

    config
}

#[task]
async fn evse(
    charger: &'static TcChargerPack<3>,
    mut pwm_tim: Peri<'static, TIM5>,
    mut pwm_pin: Peri<'static, PA0>,
    rdy_pin: Peri<'static, PC3>,
    stop_pin: Peri<'static, PC0>,
    stop_exti: Peri<'static, EXTI0>,
) {
    Evse {
        set_mains_current_limit: |max_i| charger.update_mains_limits_per_charger(max_i),
        disconnect_request: {
            let mut stop_pin = ExtiInput::new(stop_pin, stop_exti, Pull::Up, Irqs);
            async move || stop_pin.wait_for_low().await
        },
        get_pwm: {
            let timer_freq = Hertz::mhz(1);
            let mut pwm = PwmInput::new_ch1(
                pwm_tim.reborrow(),
                pwm_pin.reborrow(),
                Irqs,
                Pull::None,
                timer_freq,
            );
            pwm.enable();

            async move || {
                pwm.wait_for_period().await;

                let period = pwm.get_period_ticks();
                let width = pwm.get_width_ticks();
                if period == 0 || width > period {
                    return None;
                }

                let freq = timer_freq.0 / period;
                let freq = Frequency::from_val(freq as i32);
                let duty = (period - width) * 1000 / period;
                let duty = Ratio::from_permill(duty as u16);

                Some((freq, duty))
            }
        },
        set_rdy: {
            let mut rdy_pin = Output::new(rdy_pin, Level::Low, Speed::Low);
            move |rdy| rdy_pin.set_level(rdy.into())
        },
        get_default_current: || Current::from_val(13),
    }
    .run()
    .await;
}

struct Fan<'a, TO: GeneralInstance4Channel, TI: GeneralInstance4Channel> {
    en: Output<'a>,
    ctrl: SimplePwmChannel<'a, TO>,
    sense: PwmInput<'a, TI>,
}

impl<'a, TO: GeneralInstance4Channel, TI: GeneralInstance4Channel> Fan<'a, TO, TI> {
    fn new(en: Output<'a>, mut ctrl: SimplePwmChannel<'a, TO>, sense: PwmInput<'a, TI>) -> Self {
        ctrl.set_polarity(OutputPolarity::ActiveLow);
        let mut fan = Self { en, ctrl, sense };
        fan.set_speed(0);
        fan
    }

    fn set_speed(&mut self, speed: u8) {
        if speed > 0 {
            self.en.set_high();
            self.ctrl.enable();
            self.ctrl.set_duty_cycle_percent(speed);
        } else {
            self.en.set_low();
            self.ctrl.disable();
        }
    }
}

#[task]
async fn cooling(
    adc: Peri<'static, ADC1>,
    mut adc_pin: Peri<'static, PA1>,
    en_fan_pin: Peri<'static, PB5>,
    en_pump_pin: Peri<'static, PC12>,
    ctrl_tim: Peri<'static, TIM2>,
    ctrl_fan_pin: Peri<'static, PB3>,
    ctrl_pump_pin: Peri<'static, PA15>,
    sns_fan_tim: Peri<'static, TIM4>,
    sns_fan_pin: Peri<'static, PB6>,
    sns_pump_tim: Peri<'static, TIM1>,
    sns_pump_pin: Peri<'static, PA8>,
) {
    // Shared fan and pump ctrl timer
    let ctrl_pwm = SimplePwm::new::<AfioRemap<3>>(
        ctrl_tim,
        Some(PwmPin::new(ctrl_pump_pin, OutputType::PushPull)),
        Some(PwmPin::new(ctrl_fan_pin, OutputType::PushPull)),
        None,
        None,
        Hertz::khz(25),
        CountingMode::EdgeAlignedUp,
    )
    .split();

    // Init fan
    let sense = PwmInput::new_ch1(sns_fan_tim, sns_fan_pin, Irqs, Pull::None, Hertz::mhz(1));
    let mut fan = Fan::new(
        Output::new(en_fan_pin, Level::Low, Speed::Low),
        ctrl_pwm.ch2,
        sense,
    );

    // Init pump
    let sense = PwmInput::new_ch1::<AfioRemap<0>>(
        sns_pump_tim,
        sns_pump_pin,
        Irqs,
        Pull::None,
        Hertz::mhz(1),
    );
    let mut pump = Fan::new(
        Output::new(en_pump_pin, Level::Low, Speed::Low),
        ctrl_pwm.ch1,
        sense,
    );
    pump.set_speed(60);
    Timer::after_secs(1).await;
    pump.set_speed(40);

    // Start control loop
    CoolingController {
        setpoint: Temperature::from_val(42),
        get_adc: {
            let mut adc = Adc::new(adc);
            async move || {
                let mut adc_val = 0;
                for _ in 0..16 {
                    adc_val += adc.read(&mut adc_pin, SampleTime::CYCLES55_5).await;
                }
                adc_val
            }
        },
        set_speed: |speed| fan.set_speed(speed.as_percent()),
    }
    .run()
    .await;
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
    can_tc_charger::init(spawner, &CHARGER_PACK, p.CAN1, p.PB8, p.PB9).await;
    spawner.spawn(evse(&CHARGER_PACK, p.TIM5, p.PA0, p.PC3, p.PC0, p.EXTI0).unwrap());
    spawner.spawn(
        cooling(
            p.ADC1, p.PA1, p.PB5, p.PC12, p.TIM2, p.PB3, p.PA15, p.TIM4, p.PB6, p.TIM1, p.PA8,
        )
        .unwrap(),
    );
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
