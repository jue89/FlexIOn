#![no_std]
#![warn(unused_extern_crates)]

use core::fmt::{Debug, Formatter, Result};

use embassy_futures::select::{Either, select};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    watch::{DynReceiver, Watch},
};
use embassy_time::{Duration, Instant, Timer};

fn check_deadline<T: Clone>((deadline, val): (Instant, T)) -> Option<T> {
    if deadline > Instant::now() {
        Some(val)
    } else {
        None
    }
}

pub struct VolatileValue<T: Clone, const N: usize, const MAX_AGE_MS: u64> {
    watch: Watch<CriticalSectionRawMutex, (Instant, T), N>,
}

impl<T: Clone + Debug, const N: usize, const MAX_AGE_MS: u64> Debug
    for VolatileValue<T, N, MAX_AGE_MS>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let now = Instant::now();
        match self.watch.try_get() {
            Some((deadline, val)) => {
                if deadline > now {
                    let duration = (deadline - now).as_millis();
                    write!(f, "{:?} (expires in {}ms)", val, duration)
                } else {
                    let duration = (now - deadline).as_millis();
                    write!(f, "{:?} (expired {}ms ago)", val, duration)
                }
            }
            None => f.write_str("empty"),
        }
    }
}

impl<T: Clone, const N: usize, const MAX_AGE_MS: u64> VolatileValue<T, N, MAX_AGE_MS> {
    pub const fn new() -> Self {
        let watch = Watch::new();
        Self { watch }
    }

    pub fn update(&self, val: T) {
        let deadline = Instant::now() + Duration::from_millis(MAX_AGE_MS);
        self.watch.sender().send((deadline, val));
    }

    pub fn get(&self) -> Option<T> {
        check_deadline(self.watch.try_get()?)
    }

    pub fn observer(&self) -> Option<VolatileValueObserver<'_, T>> {
        let receiver = self.watch.dyn_receiver()?;
        Some(VolatileValueObserver {
            receiver,
            last_deadline: Instant::MIN,
        })
    }
}

impl<T: Clone, const N: usize, const MAX_AGE_MS: u64> Default for VolatileValue<T, N, MAX_AGE_MS> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct VolatileValueObserver<'a, T: Clone> {
    receiver: DynReceiver<'a, (Instant, T)>,
    last_deadline: Instant,
}

impl<'a, T: Clone> VolatileValueObserver<'a, T> {
    pub fn get(&mut self) -> Option<T> {
        check_deadline(self.receiver.try_get()?)
    }

    pub async fn change(&mut self) -> Option<T> {
        loop {
            // Lookup the current value
            let (deadline, val) = self.receiver.get().await;

            if self.last_deadline == deadline {
                // We've already seen the value ...
                if deadline > Instant::now() {
                    // ... but the value still is valid!
                    // -> Race between new value and expiry
                    match select(Timer::at(deadline), self.receiver.changed()).await {
                        Either::First(()) => return None,
                        Either::Second(_) => continue,
                    }
                } else {
                    // ... the value expired. Wait for an update.
                    self.receiver.changed().await;
                    continue;
                }
            } else {
                // New value!
                // Mark it seen by recording its deadline
                self.last_deadline = deadline;
                if deadline > Instant::now() {
                    // The value still is fresh ... return it!
                    return Some(val);
                } else {
                    // It's an old value ... ignore it
                    continue;
                }
            }
        }
    }
}
