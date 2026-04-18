use embassy_executor::{Spawner, task};
use embassy_stm32::{
    Peri, bind_interrupts,
    dma::InterruptHandler as DmaInterruptHandler,
    mode::Async,
    peripherals::{DMA1_CH4, DMA1_CH5, PA9, PA10, USART1},
    usart::{Config, InterruptHandler as UsartInterruptHandler, Uart, UartRx, UartTx},
};
use log_to_uart::Logger;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USART1 => UsartInterruptHandler<USART1>;
    DMA1_CHANNEL4 => DmaInterruptHandler<DMA1_CH4>;
    DMA1_CHANNEL5 => DmaInterruptHandler<DMA1_CH5>;
});

const BUFFER_SIZE: usize = 4096;

#[task]
async fn logger_task(
    logger: &'static Logger<BUFFER_SIZE>,
    tx: &'static mut UartTx<'static, Async>,
    rx: &'static mut UartRx<'static, Async>,
) {
    logger
        .run(
            async |buf| {
                let _ = tx.write(buf).await;
            },
            async || {
                let mut buf = [0u8];
                while let Err(_) = rx.read(&mut buf).await {}
                buf[0]
            },
        )
        .await;
}

pub fn init(
    spawner: Spawner,
    uart: Peri<'static, USART1>,
    tx: Peri<'static, PA9>,
    rx: Peri<'static, PA10>,
    tx_dma: Peri<'static, DMA1_CH4>,
    rx_dma: Peri<'static, DMA1_CH5>,
) {
    static LOGGER: Logger<BUFFER_SIZE> = Logger::new();

    let config = {
        let mut config = Config::default();
        config.baudrate = 115200;
        config
    };

    let (uart_tx, uart_rx) = {
        static CELL: StaticCell<Uart<Async>> = StaticCell::new();
        CELL.init(Uart::new(uart, rx, tx, tx_dma, rx_dma, Irqs, config).unwrap())
            .split_ref()
    };

    spawner.spawn(logger_task(&LOGGER, uart_tx, uart_rx).unwrap());
}
