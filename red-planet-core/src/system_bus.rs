use crate::bus::Bus;
use crate::Allocator;
use core::fmt;
use std::fmt::Debug;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AccessType {
    /// Regular reads.
    Read,
    /// Regular writes.
    Write,
    /// Instruction fetches.
    Execute,
}

impl fmt::Display for AccessType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::Read => "R",
            Self::Write => "W",
            Self::Execute => "X",
        })
    }
}

pub trait SystemBus<A: Allocator>: Bus<A> {
    fn accepts(&self, address: u32, size: usize, access_type: AccessType) -> bool;
}
