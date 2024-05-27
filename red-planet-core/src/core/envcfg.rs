#![allow(unused)]

use bitvec::{order::Lsb0, view::BitView};
use space_time::allocator::Allocator;

use crate::system_bus::SystemBus;

use super::Core;

/// Provides menvcfg, menvcfgh, and senvcfg registers.
#[derive(Debug, Clone)]
pub struct Envcfg {
    menvcfg: u32,
    menvcfgh: u32,
    senvcfg: u32,
}

impl Default for Envcfg {
    fn default() -> Self {
        Self::new()
    }
}

impl Envcfg {
    pub fn new() -> Self {
        Self {
            menvcfg: 0x0000_0000,
            menvcfgh: 0x0000_0000,
            senvcfg: 0x0000_0000,
        }
    }

    pub fn m_fiom(&self) -> bool {
        self.menvcfg.view_bits::<Lsb0>()[idx::FIOM]
    }

    pub fn set_m_fiom(&mut self, value: bool) {
        self.menvcfg.view_bits_mut::<Lsb0>().set(idx::FIOM, value);
    }

    pub fn s_fiom(&self) -> bool {
        self.senvcfg.view_bits::<Lsb0>()[idx::FIOM]
    }

    pub fn set_s_fiom(&mut self, value: bool) {
        self.senvcfg.view_bits_mut::<Lsb0>().set(idx::FIOM, value);
    }

    pub fn pbmte(&self) -> bool {
        self.menvcfgh.view_bits::<Lsb0>()[hidx::PBMTE]
    }

    pub fn set_pbmte(&mut self, value: bool) {
        self.menvcfgh
            .view_bits_mut::<Lsb0>()
            .set(hidx::PBMTE, value);
    }
}

/// Bit indices for the fields of the menvcfg register.
mod idx {
    pub const FIOM: usize = 0;
    // The meaning of the following fields is not yet defined in the latest spec.
    // const CBIE: usize = 4;
    // const CBCFE: usize = 6;
    // const CBZE: usize = 7;
}

/// Bit indices for the fields of the menvcfgh register.
mod hidx {
    pub const PBMTE: usize = 30;
    // The meaning of the following fields is not yet defined in the latest spec.
    // const STCE: usize = 31;
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    pub fn read_menvcfg(&self, allocator: &mut A) -> u32 {
        self.envcfg.get(allocator).menvcfg
    }

    pub fn write_menvcfg(&self, allocator: &mut A, value: u32, mask: u32) {
        let menvcfg = &mut self.envcfg.get_mut(allocator).menvcfg;
        *menvcfg = *menvcfg & !mask | value & mask;
    }

    pub fn read_menvcfgh(&self, allocator: &mut A) -> u32 {
        self.envcfg.get(allocator).menvcfgh
    }

    pub fn write_menvcfgh(&self, allocator: &mut A, value: u32, mask: u32) {
        let menvcfgh = &mut self.envcfg.get_mut(allocator).menvcfgh;
        *menvcfgh = *menvcfgh & !mask | value & mask;
    }

    pub fn read_senvcfg(&self, allocator: &mut A) -> u32 {
        self.envcfg.get(allocator).senvcfg
    }

    pub fn write_senvcfg(&self, allocator: &mut A, value: u32, mask: u32) {
        let senvcfg = &mut self.envcfg.get_mut(allocator).senvcfg;
        *senvcfg = *senvcfg & !mask | value & mask;
    }
}
