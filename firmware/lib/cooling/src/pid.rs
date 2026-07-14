use core::ops::Range;

use embassy_time::Instant;
use log::trace;

pub struct Pid {
    last_val: Option<Instant>,
    setpoint: i32,
    err_acc: i32,
    k_p: i32,
    k_i: i32,
    u_range: Range<i32>,
}

impl Pid {
    pub fn new() -> Self {
        Self {
            last_val: None,
            setpoint: 0,
            err_acc: 0,
            k_p: 0,
            k_i: 0,
            u_range: i32::MIN..i32::MAX,
        }
    }

    pub fn with_k_p(mut self, k_p: i32) -> Self {
        self.k_p = k_p;
        self
    }

    pub fn with_k_i(mut self, k_i: i32) -> Self {
        self.k_i = k_i;
        self
    }

    pub fn with_range(mut self, u_range: Range<i32>) -> Self {
        self.u_range = u_range;
        self
    }

    pub fn with_setpoint(mut self, setpoint: i32) -> Self {
        self.setpoint = setpoint;
        self
    }

    pub fn step(&mut self, val: i32) -> i32 {
        // P
        let err_p = self.setpoint.saturating_sub(val);
        let u_p = err_p.saturating_mul(self.k_p);

        // dt
        let now = Instant::now();
        let dt = if let Some(last_val) = self.last_val {
            (now - last_val).as_secs() as i32
        } else {
            0
        };
        self.last_val = Some(now);

        // I
        let err_i = self.err_acc.saturating_add(err_p.saturating_mul(dt));
        let u_i = err_i.saturating_mul(self.k_i);

        trace!(
            "err_p = {}, u_p = {}, err_i = {}, u_i = {}",
            err_p, u_p, err_i, u_i
        );

        // u
        let u = u_p.strict_add(u_i);
        if u <= self.u_range.start {
            self.u_range.start
        } else if u >= self.u_range.end {
            self.u_range.end
        } else {
            // Only store the integrated error if u hasn't reached its bounds
            // Anti-windup
            self.err_acc = err_i;
            u
        }
    }
}
