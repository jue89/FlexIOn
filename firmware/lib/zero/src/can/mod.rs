pub mod msgs;
pub mod types;

use core::fmt::Debug;

use crate::state::ZeroState;

pub trait Msg: Sized + Debug {
    fn handle(&self, state: &ZeroState);
}
