use core::fmt::{Debug, Display};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ratio(pub(crate) u16);

impl Ratio {
    pub(crate) const MAX: u16 = 100 * 100;
    pub const ZERO: Self = Self(0);

    const fn new(mut val: u16) -> Self {
        if val > Self::MAX {
            val = Self::MAX
        }
        Self(val)
    }

    pub const fn from_percent(percent: u8) -> Self {
        Self::new(percent as u16 * 100)
    }

    pub const fn from_permill(permill: u16) -> Self {
        Self::new(permill * 10)
    }

    pub const fn as_percent(&self) -> u8 {
        (self.0 / 100) as u8
    }

    pub const fn as_permill(&self) -> u16 {
        self.0 / 10
    }
}

impl Display for Ratio {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let a = self.0 / 100;
        let b = self.0 % 100;
        write!(f, "{}.{:02}%", a, b)
    }
}

impl Debug for Ratio {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let a = self.0 / 100;
        let b = self.0 % 100;
        write!(f, "{}.{:02}%", a, b)
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Ratio {
    fn format(&self, fmt: defmt::Formatter) {
        let a = self.0 / 100;
        let b = self.0 % 100;
        defmt::write!(fmt, "{}.{:02}%", a, b)
    }
}
