use embassy_executor::{Spawner, task};
use embassy_stm32::{
    Peri, bind_interrupts,
    can::{
        Can, CanRx, CanTx, Fifo, Frame, Id, Rx0InterruptHandler, Rx1InterruptHandler,
        SceInterruptHandler, TxInterruptHandler, filter,
    },
    peripherals::{CAN1, PB8, PB9},
};
use log::trace;
use static_cell::StaticCell;
use tc_charger::{
    self, TcChargerPack,
    can::{CanPayload, RxMsg, TxMsg},
};
use zero::ZeroState;

bind_interrupts!(struct Can1Irqs {
    CAN1_RX0 => Rx0InterruptHandler<CAN1>;
    CAN1_RX1 => Rx1InterruptHandler<CAN1>;
    CAN1_SCE => SceInterruptHandler<CAN1>;
    CAN1_TX => TxInterruptHandler<CAN1>;
});

pub async fn init(
    spawner: Spawner,
    chargers: &'static TcChargerPack<3>,
    zero: &'static ZeroState,
    peri: Peri<'static, CAN1>,
    rx: Peri<'static, PB8>,
    tx: Peri<'static, PB9>,
) {
    let (tc_can_tx, tc_can_rx) = {
        static TC_CAN: StaticCell<Can<'static>> = StaticCell::new();
        let tc_can = TC_CAN.init(Can::new(peri, rx, tx, Can1Irqs));
        tc_can
            .modify_filters()
            .enable_bank(0, Fifo::Fifo0, filter::Mask32::accept_all())
            // CAN2 FILTERS MUST BE ENABLED HERE!
            .slave_filters()
            .enable_bank(14, Fifo::Fifo0, filter::Mask32::accept_all());
        tc_can
            .modify_config()
            .set_loopback(false)
            .set_silent(false)
            .set_bitrate(250_000);
        tc_can.enable().await;
        tc_can.split()
    };

    spawner.spawn(tc_charger_task(chargers, zero).unwrap());
    spawner.spawn(tc_charger_can_tx(chargers, tc_can_tx).unwrap());
    spawner.spawn(tc_charger_can_rx(chargers, tc_can_rx).unwrap());
}

#[task]
async fn tc_charger_task(chargers: &'static TcChargerPack<3>, zero: &'static ZeroState) {
    chargers.task(|| zero.pack_voltage.get()).await;
}

#[task]
async fn tc_charger_can_tx(chargers: &'static TcChargerPack<3>, mut can_tx: CanTx<'static>) {
    loop {
        let TxMsg { id, payload } = chargers.next_can_tx_msg().await;
        let payload: CanPayload = payload.into();
        trace!("TCCAN TX 0x{:X}: {:?}", id, payload);
        let frm = Frame::new_extended(id, &payload[..]).unwrap();
        can_tx.write(&frm).await;
    }
}

#[task]
async fn tc_charger_can_rx(chargers: &'static TcChargerPack<3>, mut can_rx: CanRx<'static>) {
    loop {
        let Ok(envelope) = can_rx.read().await else {
            continue;
        };
        let frame = envelope.frame;
        let Id::Extended(id) = frame.id() else {
            continue;
        };
        let id = id.as_raw();
        trace!("TCCAN RX 0x{:X}: {:?}", id, frame.data());
        let Ok(payload) = frame.data().try_into() else {
            continue;
        };
        chargers.handle_can_msg(RxMsg { id, payload });
    }
}
