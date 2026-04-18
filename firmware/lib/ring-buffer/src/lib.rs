#![cfg_attr(not(test), no_std)]
#![warn(unused_extern_crates)]

mod cell;

use core::{
    fmt,
    future::poll_fn,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::Poll,
};

use embassy_sync::waitqueue::AtomicWaker;

use crate::cell::SyncUnsafeCell;

pub struct RingBuffer<const SIZE: usize> {
    read_cnt: AtomicUsize,
    write_cnt: AtomicUsize,
    read_waker: AtomicWaker,
    write_waker: AtomicWaker,
    reader_active: AtomicBool,
    writer_active: AtomicBool,
    buffer: SyncUnsafeCell<[u8; SIZE]>,
}

impl<const SIZE: usize> RingBuffer<SIZE> {
    pub const fn new() -> Self {
        assert!(usize::MAX % SIZE == SIZE - 1, "SIZE must be power of two");
        Self {
            read_cnt: AtomicUsize::new(0),
            write_cnt: AtomicUsize::new(0),
            read_waker: AtomicWaker::new(),
            write_waker: AtomicWaker::new(),
            reader_active: AtomicBool::new(false),
            writer_active: AtomicBool::new(false),
            buffer: SyncUnsafeCell::new([0u8; SIZE]),
        }
    }

    pub fn reader<'a>(&'a self) -> Option<RingBufferReader<'a, SIZE>> {
        let reader_was_active = self.reader_active.swap(true, Ordering::Acquire);
        if reader_was_active {
            None
        } else {
            Some(RingBufferReader { rb: self })
        }
    }

    pub fn writer<'a>(&'a self) -> Option<RingBufferWriter<'a, SIZE>> {
        let writer_was_active = self.writer_active.swap(true, Ordering::Acquire);
        if writer_was_active {
            None
        } else {
            Some(RingBufferWriter { rb: self })
        }
    }
}

pub struct RingBufferWriter<'a, const SIZE: usize> {
    rb: &'a RingBuffer<SIZE>,
}

impl<'a, const SIZE: usize> Drop for RingBufferWriter<'a, SIZE> {
    fn drop(&mut self) {
        self.rb.writer_active.store(false, Ordering::Release);
    }
}

impl<'a, const SIZE: usize> RingBufferWriter<'a, SIZE> {
    #[allow(clippy::mut_from_ref)]
    fn get_write_buffer(&self) -> Option<&mut [u8]> {
        // Get the writable len of the buffer
        let write_cnt = self.rb.write_cnt.load(Ordering::Acquire);
        let read_cnt = self.rb.read_cnt.load(Ordering::Acquire);
        let writable_len = SIZE - write_cnt.wrapping_sub(read_cnt);
        if writable_len == 0 {
            return None;
        }

        // Slice the writable part of the buffer
        let start_offset = write_cnt % SIZE;
        let end_offset = (start_offset + writable_len).min(SIZE);
        let buffer = unsafe { &mut *self.rb.buffer.get() };
        Some(&mut buffer[start_offset..end_offset])
    }

    fn advance_write_cnt(&self, written_bytes: usize) {
        // Move write pointer
        self.rb
            .write_cnt
            .fetch_add(written_bytes, Ordering::Release);

        // Wake waiting readers
        self.rb.read_waker.wake();
    }

    pub fn try_write(&mut self, cb: impl FnOnce(&mut [u8]) -> usize) -> Option<usize> {
        // Ask the callback to fill the buffer
        // It returns the amount of written bytes
        let written_bytes = cb(self.get_write_buffer()?);

        self.advance_write_cnt(written_bytes);

        Some(written_bytes)
    }

    pub async fn write(&mut self, cb: impl FnOnce(&mut [u8]) -> usize) -> usize {
        let write_buffer = poll_fn(|cx| match self.get_write_buffer() {
            Some(buf) => Poll::Ready(buf),
            None => {
                self.rb.write_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await;

        // Ask the callback to fill the buffer
        // It returns the amount of written bytes
        let written_bytes = cb(write_buffer);

        self.advance_write_cnt(written_bytes);

        written_bytes
    }
}

impl<'a, const BUFFER_SIZE: usize> fmt::Write for RingBufferWriter<'a, BUFFER_SIZE> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut s = s.as_bytes();
        while !s.is_empty() {
            if let Some(written) = self.try_write(|buffer| {
                let len = s.len().min(buffer.len());
                buffer[..len].copy_from_slice(&s[..len]);
                len
            }) {
                s = &s[written..];
            } else {
                break;
            };
        }
        Ok(())
    }
}

pub struct RingBufferReader<'a, const SIZE: usize> {
    rb: &'a RingBuffer<SIZE>,
}

impl<'a, const SIZE: usize> Drop for RingBufferReader<'a, SIZE> {
    fn drop(&mut self) {
        self.rb.reader_active.store(false, Ordering::Release);
    }
}

impl<'a, const SIZE: usize> RingBufferReader<'a, SIZE> {
    fn get_read_buffer(&self) -> Option<&[u8]> {
        let write_cnt = self.rb.write_cnt.load(Ordering::Acquire);
        let read_cnt = self.rb.read_cnt.load(Ordering::Acquire);
        let readable_len = write_cnt.wrapping_sub(read_cnt);
        if readable_len == 0 {
            return None;
        }

        let start_offset = read_cnt % SIZE;
        let end_offset = (start_offset + readable_len).min(SIZE);
        let buffer = unsafe { &*self.rb.buffer.get() };
        Some(&buffer[start_offset..end_offset])
    }

    fn advance_read_cnt(&self, read_bytes: usize) {
        self.rb.read_cnt.fetch_add(read_bytes, Ordering::Release);

        // Wake waiting writer
        self.rb.write_waker.wake();
    }

    pub fn clear(&mut self) {
        let write_cnt = self.rb.write_cnt.load(Ordering::Acquire);
        self.rb.read_cnt.store(write_cnt, Ordering::Release);
    }

    pub fn try_read<R>(&mut self, cb: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let buffer = self.get_read_buffer()?;
        let ret = cb(buffer);
        self.advance_read_cnt(buffer.len());
        Some(ret)
    }

    pub async fn read<R>(&mut self, cb: impl AsyncFnOnce(&[u8]) -> R) -> R {
        // Wait until somthing is readable
        let buffer = poll_fn(|cx| match self.get_read_buffer() {
            Some(buf) => Poll::Ready(buf),
            None => {
                self.rb.read_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await;

        // Let the callback consume the data
        let ret = cb(buffer).await;

        self.advance_read_cnt(buffer.len());

        ret
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::Ordering, usize};

    use embassy_futures::join::join;

    use crate::RingBuffer;

    #[test]
    fn ring_buffer() {
        let rb = RingBuffer::<4>::new();
        let mut reader = rb.reader().unwrap();
        let mut writer = rb.writer().unwrap();

        // Nothin to read
        assert!(reader.try_read(|_| ()).is_none());

        // Write and read a byte
        let ret = writer.try_write(|buf| {
            buf[0] = 123;
            1
        });
        assert_eq!(ret, Some(1));
        let ret = reader.try_read(|buf| {
            assert_eq!(buf, &[123]);
            42
        });
        assert_eq!(ret, Some(42));

        // Write further bytes
        writer.try_write(|buf| {
            assert_eq!(buf.len(), 3);
            buf.fill(42);
            buf.len()
        });
        writer.try_write(|buf| {
            assert_eq!(buf.len(), 1);
            buf.fill(43);
            buf.len()
        });
        assert!(writer.try_write(|_| 0).is_none());

        // Read all bytes
        reader.try_read(|buf| {
            assert_eq!(buf, &[42, 42, 42]);
        });
        reader.try_read(|buf| {
            assert_eq!(buf, &[43]);
        });
    }

    #[test]
    fn ensure_spsc() {
        let rb = RingBuffer::<4>::new();
        {
            let r1 = rb.reader();
            assert!(r1.is_some());
            let r2 = rb.reader();
            assert!(r2.is_none());
            let w1 = rb.writer();
            assert!(w1.is_some());
            let w2 = rb.writer();
            assert!(w2.is_none());
        }
        let r3 = rb.reader();
        assert!(r3.is_some());
        let w3 = rb.writer();
        assert!(w3.is_some());
    }

    #[test]
    fn handle_usize_overflow() {
        let rb = RingBuffer::<4>::new();
        rb.read_cnt.store(usize::MAX, Ordering::Release);
        rb.write_cnt.store(usize::MAX, Ordering::Release);

        let mut writer = rb.writer().unwrap();
        writer.try_write(|buf| {
            assert_eq!(buf.len(), 1);
            buf.len()
        });
        writer.try_write(|buf| {
            assert_eq!(buf.len(), 3);
            buf.len()
        });

        let mut reader = rb.reader().unwrap();
        reader.try_read(|buf| {
            assert_eq!(buf.len(), 1);
        });
        reader.try_read(|buf| {
            assert_eq!(buf.len(), 3);
        });
    }

    #[embassy_unittest::test]
    async fn async_read_write() {
        let rb = RingBuffer::<4>::new();
        let test_range = 0..=u8::MAX;
        join(
            async {
                let mut writer = rb.writer().unwrap();
                for i in test_range.clone() {
                    writer
                        .write(|buf| {
                            buf[0] = i;
                            1
                        })
                        .await;
                }
            },
            async {
                let mut reader = rb.reader().unwrap();
                let mut range = test_range.clone().into_iter();
                loop {
                    let done = reader
                        .read(async |buf| {
                            for i in buf {
                                assert_eq!(*i, range.next().unwrap())
                            }
                            range.is_empty()
                        })
                        .await;
                    if done {
                        break;
                    }
                }
            },
        )
        .await;
    }
}
