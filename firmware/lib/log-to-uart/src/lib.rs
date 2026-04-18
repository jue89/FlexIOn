#![no_std]
#![warn(unused_extern_crates)]

use core::{cell::RefCell, fmt::Write as _};

use embassy_futures::select::select;
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use embassy_time::Instant;
use heapless::String;
use log::{Level, LevelFilter, Log, Metadata, Record, set_logger, set_max_level};
use ring_buffer::RingBuffer;

pub struct Logger<const BUFFER_SIZE: usize> {
    filter: Mutex<CriticalSectionRawMutex, RefCell<String<32>>>,
    rb: RingBuffer<BUFFER_SIZE>,
}

impl<const BUFFER_SIZE: usize> Logger<BUFFER_SIZE> {
    pub const fn new() -> Self {
        let filter = Mutex::new(RefCell::new(String::new()));
        let rb = RingBuffer::new();
        Self { filter, rb }
    }

    pub async fn run(
        &'static self,
        mut tx: impl AsyncFnMut(&[u8]),
        mut rx: impl AsyncFnMut() -> u8,
    ) {
        let mut reader = self.rb.reader().unwrap();

        set_logger(self).unwrap();
        set_max_level(LevelFilter::Info);

        loop {
            // Clear current line
            tx(b"\x1b[2K\r").await;

            // Start logging
            select(
                async {
                    loop {
                        let _ = reader.read(async |data| tx(data).await).await;
                    }
                },
                async {
                    loop {
                        let level = match rx().await {
                            b'0' => LevelFilter::Off,
                            b'1' => LevelFilter::Error,
                            b'2' => LevelFilter::Warn,
                            b'3' => LevelFilter::Info,
                            b'4' => LevelFilter::Debug,
                            b'5' => LevelFilter::Trace,
                            b'?' => break,
                            _ => continue,
                        };
                        set_max_level(level);
                    }
                },
            )
            .await;

            // Ask for filter
            loop {
                // Output current filter
                let mut line = String::<48>::new();
                self.filter.lock(|filter| {
                    let _ = write!(&mut line, "\x1b[2K\rFILTER> {}", filter.borrow());
                });
                tx(line.as_bytes()).await;

                // Read key-stroke
                let key = rx().await;

                // Filter confirmed
                if key == b'\r' {
                    break;
                }

                // Modify filter
                self.filter.lock(|filter| {
                    let mut filter = filter.borrow_mut();
                    if key == b'\x08' || key == b'\x7f' {
                        filter.pop();
                    } else {
                        let _ = filter.push(key.into());
                    }
                });
            }

            // Clear buffer
            reader.clear();
        }
    }
}

impl<const N: usize> Log for Logger<N> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let target = metadata.target();
        self.filter
            .lock(|filter| target.starts_with(filter.borrow().as_str()))
    }

    fn log(&self, record: &Record) {
        if let Some(mut writer) = self.rb.writer() {
            if !self.enabled(record.metadata()) {
                return;
            }

            let target = record.metadata().target();
            let millis = Instant::now().as_millis();
            let secs = millis / 1000;
            let millis = millis % 1000;
            let level = match record.metadata().level() {
                Level::Error => "\x1b[0;91mE\x1b[0m",
                Level::Warn => "\x1b[0;93mW\x1b[0m",
                Level::Info => "\x1b[0;92mI\x1b[0m",
                Level::Debug => "\x1b[0;97mD\x1b[0m",
                Level::Trace => "\x1b[0;90mT\x1b[0m",
            };
            let _ = write!(
                &mut writer,
                "{:4}.{:03} {} \x1b[1;90m{}\x1b[0m {}\r\n",
                secs,
                millis,
                level,
                target,
                record.args(),
            );
        }
    }

    fn flush(&self) {}
}
