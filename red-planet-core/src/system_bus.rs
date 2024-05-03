use crate::bus::Bus;
use crate::Allocator;
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

pub trait SystemBus<A: Allocator>: Bus<A> {
    fn accepts(&self, address: u32, size: usize, access_type: AccessType) -> bool;
}
