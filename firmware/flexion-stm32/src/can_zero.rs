use embassy_executor::{Spawner, task};
use embassy_stm32::{
    Peri, bind_interrupts,
    can::{
        Can, Id, Rx0InterruptHandler, Rx1InterruptHandler, SceInterruptHandler, TxInterruptHandler,
    },
    peripherals::{CAN2, PB12, PB13},
};
use log::trace;
use static_cell::StaticCell;
use zero::{self, ZeroState};

bind_interrupts!(struct Can2Irqs {
    CAN2_RX0 => Rx0InterruptHandler<CAN2>;
    CAN2_RX1 => Rx1InterruptHandler<CAN2>;
    CAN2_SCE => SceInterruptHandler<CAN2>;
    CAN2_TX => TxInterruptHandler<CAN2>;
});

pub async fn init(
    spawner: Spawner,
    zero_state: &'static ZeroState,
    peri: Peri<'static, CAN2>,
    rx: Peri<'static, PB12>,
    tx: Peri<'static, PB13>,
) {
    let can = {
        static ZERO_CAN: StaticCell<Can<'static>> = StaticCell::new();
        let can = ZERO_CAN.init(Can::new(peri, rx, tx, Can2Irqs));
        // WARNING! FILTERS FOR THIS CAN INTERFACE ARE ENABLED
        // DURING CAN1 SETUP!
        can.modify_config()
            .set_loopback(false)
            .set_silent(false)
            .set_bitrate(500_000);
        can.enable().await;
        can
    };

    spawner.spawn(zero_can_rx(can, zero_state).unwrap());
}

#[task]
async fn zero_can_rx(can: &'static mut Can<'static>, zero_state: &'static ZeroState) {
    loop {
        if let Ok(envelope) = can.read().await {
            let frame = envelope.frame;
            let Id::Standard(id) = frame.id() else {
                continue;
            };
            let id = id.as_raw() as u32;
            trace!("ZEROCAN RX 0x{:X}: {:?}", id, frame.data());
            zero_state.handle_can_msg(id, frame.data());
        }
    }
}
