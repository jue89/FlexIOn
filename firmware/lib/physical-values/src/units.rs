pub trait Unit {
    const NAME: &'static str;
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Second();
impl Unit for Second {
    const NAME: &'static str = "s";
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hz();
impl Unit for Hz {
    const NAME: &'static str = "Hz";
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Volt();
impl Unit for Volt {
    const NAME: &'static str = "V";
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ampere();
impl Unit for Ampere {
    const NAME: &'static str = "A";
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Watt();
impl Unit for Watt {
    const NAME: &'static str = "W";
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AmpereHour();
impl Unit for AmpereHour {
    const NAME: &'static str = "Ah";
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DegCelsius();
impl Unit for DegCelsius {
    const NAME: &'static str = "°C";
}
