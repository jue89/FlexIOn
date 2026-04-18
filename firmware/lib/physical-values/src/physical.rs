use core::{
    fmt::{Debug, Display},
    marker::PhantomData,
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Range, Sub, SubAssign},
};

use crate::{Ratio, units::Unit};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Physical<U: Unit> {
    pub(crate) millis: i32,
    _unit: PhantomData<U>,
}

impl<U: Unit> Physical<U> {
    pub const ZERO: Self = Self::from_millis(0);

    pub const fn from_millis(millis: i32) -> Self {
        Self {
            millis,
            _unit: PhantomData,
        }
    }

    pub const fn from_val(val: i32) -> Self {
        Self::from_millis(val.saturating_mul(1000))
    }

    pub const fn from_decimal(mut val: i32, mut dec_places: usize) -> Self {
        while dec_places < 3 {
            val = val.saturating_mul(10);
            dec_places += 1;
        }

        while dec_places > 3 {
            val = val.saturating_div(10);
            dec_places -= 1;
        }

        Self::from_millis(val)
    }

    pub const fn as_millis(&self) -> i32 {
        self.millis
    }

    pub const fn as_val(&self) -> i32 {
        self.millis / 1000
    }

    pub const fn as_decimal(&self, dec_places: usize) -> i32 {
        let mut cur_dec_places = 3;
        let mut val = self.millis;

        while cur_dec_places > dec_places {
            val = val.saturating_div(10);
            cur_dec_places -= 1;
        }

        while cur_dec_places < dec_places {
            val = val.saturating_mul(10);
            cur_dec_places += 1;
        }

        val
    }

    pub fn as_range(&self, tolerance: Ratio) -> Range<Self>
    where
        Self: Copy,
    {
        let diff = *self * tolerance;
        Range {
            start: *self - diff,
            end: *self + diff,
        }
    }

    pub const fn frac(&self, p: i32, q: u32) -> Self {
        let mut val = self.millis as i64;
        val = val.saturating_mul(p as i64);
        val = val.saturating_div(q as i64);
        Self::from_millis(val as i32)
    }
}

impl<U: Unit> Add for Physical<U> {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self::Output {
        self.millis += rhs.millis;
        self
    }
}

impl<U: Unit> AddAssign for Physical<U> {
    fn add_assign(&mut self, rhs: Self) {
        self.millis += rhs.millis
    }
}

impl<U: Unit> Sub for Physical<U> {
    type Output = Self;

    fn sub(mut self, rhs: Self) -> Self::Output {
        self.millis -= rhs.millis;
        self
    }
}

impl<U: Unit> SubAssign for Physical<U> {
    fn sub_assign(&mut self, rhs: Self) {
        self.millis -= rhs.millis;
    }
}

impl<U: Unit> Neg for Physical<U> {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::from_millis(-self.as_millis())
    }
}

impl<U: Unit> Mul<usize> for Physical<U> {
    type Output = Self;

    fn mul(mut self, rhs: usize) -> Self::Output {
        self.millis *= rhs as i32;
        self
    }
}

impl<U: Unit> Div<usize> for Physical<U> {
    type Output = Self;

    fn div(mut self, rhs: usize) -> Self::Output {
        self.millis /= rhs as i32;
        self
    }
}

impl<U: Unit> MulAssign<usize> for Physical<U> {
    fn mul_assign(&mut self, rhs: usize) {
        self.millis *= rhs as i32
    }
}

impl<U: Unit> DivAssign<usize> for Physical<U> {
    fn div_assign(&mut self, rhs: usize) {
        self.millis /= rhs as i32
    }
}

impl<U: Unit> Mul<Ratio> for Physical<U> {
    type Output = Self;

    fn mul(self, rhs: Ratio) -> Self::Output {
        self.frac(rhs.0 as i32, Ratio::MAX as u32)
    }
}

impl<U: Unit> Div<Ratio> for Physical<U> {
    type Output = Self;

    fn div(self, rhs: Ratio) -> Self::Output {
        self.frac(Ratio::MAX as i32, rhs.0 as u32)
    }
}

impl<U: Unit> MulAssign<Ratio> for Physical<U> {
    fn mul_assign(&mut self, rhs: Ratio) {
        *self = self.frac(rhs.0 as i32, Ratio::MAX as u32);
    }
}

impl<U: Unit> DivAssign<Ratio> for Physical<U> {
    fn div_assign(&mut self, rhs: Ratio) {
        *self = self.frac(Ratio::MAX as i32, rhs.0 as u32);
    }
}

impl<U: Unit> Display for Physical<U> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let sign = if self.millis < 0 { '-' } else { '+' };
        let a = (self.millis / 1000).unsigned_abs();
        let b = (self.millis % 1000).unsigned_abs();
        write!(f, "{}{}.{:03}{}", sign, a, b, U::NAME)
    }
}

impl<U: Unit> Debug for Physical<U> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let sign = if self.millis < 0 { '-' } else { '+' };
        let a = (self.millis / 1000).unsigned_abs();
        let b = (self.millis % 1000).unsigned_abs();
        write!(f, "{}{}.{:03}{}", sign, a, b, U::NAME)
    }
}

#[cfg(feature = "defmt")]
impl<U: Unit> defmt::Format for Physical<U> {
    fn format(&self, fmt: defmt::Formatter) {
        let sign = if self.millis < 0 { '-' } else { '+' };
        let a = (self.millis / 1000).abs() as u32;
        let b = (self.millis % 1000).abs() as u16;
        defmt::write!(fmt, "{}{}.{:03}{}", sign, a, b, U::NAME)
    }
}
