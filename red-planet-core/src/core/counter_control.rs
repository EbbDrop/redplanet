use bitvec::{order::Lsb0, view::BitView};
use space_time::allocator::Allocator;

use crate::system_bus::SystemBus;

use super::Core;

#[derive(Debug, Clone)]
pub struct CounterControl {
    pub mcounteren: Counteren,
    pub scounteren: Counteren,
    pub mcountinhibit: Mcountinhibit,
}

impl Default for CounterControl {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterControl {
    pub fn new() -> Self {
        Self {
            mcounteren: Counteren::new(),
            scounteren: Counteren::new(),
            mcountinhibit: Mcountinhibit::new(),
        }
    }
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    pub fn read_mcounteren(&self, allocator: &mut A) -> u32 {
        self.counter_control.get(allocator).mcounteren.read()
    }

    pub fn write_mcounteren(&self, allocator: &mut A, value: u32, mask: u32) {
        let counter_control = self.counter_control.get_mut(allocator);
        counter_control.mcounteren.write(value, mask);
    }

    pub fn read_scounteren(&self, allocator: &mut A) -> u32 {
        self.counter_control.get(allocator).scounteren.read()
    }

    pub fn write_scounteren(&self, allocator: &mut A, value: u32, mask: u32) {
        let counter_control = self.counter_control.get_mut(allocator);
        counter_control.scounteren.write(value, mask);
    }

    pub fn read_mcountinhibit(&self, allocator: &mut A) -> u32 {
        self.counter_control.get(allocator).mcountinhibit.read()
    }

    pub fn write_mcountinhibit(&self, allocator: &mut A, value: u32, mask: u32) {
        let counter_control = self.counter_control.get_mut(allocator);
        counter_control.mcountinhibit.write(value, mask);
    }
}

/// Counter-Enable register (mcounteren and scounteren).
///
/// All fields of the register are **WARL**.
#[derive(Debug, Clone)]
pub struct Counteren(u32);

impl Default for Counteren {
    fn default() -> Self {
        Self::new()
    }
}

impl Counteren {
    #![allow(dead_code)] // TODO

    // Bit indices for the fields of the counter-enable register.
    // Indicies 3 -> 31 map to HPM3 -> HPM31.
    const CY: usize = 0;
    const TM: usize = 1;
    const IR: usize = 2;

    pub fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub fn cy(&self) -> bool {
        self.0.view_bits::<Lsb0>()[Self::CY]
    }

    pub fn set_cy(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(Self::CY, value)
    }

    pub fn tm(&self) -> bool {
        self.0.view_bits::<Lsb0>()[Self::TM]
    }

    pub fn set_tm(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(Self::TM, value)
    }

    pub fn ir(&self) -> bool {
        self.0.view_bits::<Lsb0>()[Self::IR]
    }

    pub fn set_ir(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(Self::IR, value)
    }

    pub fn hpm(&self, n: u8) -> bool {
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        self.0.view_bits::<Lsb0>()[n as usize]
    }

    pub fn set_hpm(&mut self, n: u8, value: bool) {
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        self.0.view_bits_mut::<Lsb0>().set(n as usize, value)
    }

    fn read(&self) -> u32 {
        self.0
    }

    fn write(&mut self, value: u32, mask: u32) {
        self.0 = self.0 & !mask | value & mask;
    }
}

/// The mcountinhibit register is **WARL**.
#[derive(Debug, Clone)]
pub struct Mcountinhibit(u32);

impl Default for Mcountinhibit {
    fn default() -> Self {
        Self::new()
    }
}

impl Mcountinhibit {
    #![allow(dead_code)] // TODO

    // Bit indices for the fields of the mcountinhibit register.
    // Index 1 is a read-only zero bit.
    // Indicies 3 -> 31 map to HPM3 -> HPM31.
    const CY: usize = 0;
    const IR: usize = 2;

    pub fn new() -> Self {
        Self(0x0000_0000)
    }

    pub fn cy(&self) -> bool {
        self.0.view_bits::<Lsb0>()[Self::CY]
    }

    pub fn set_cy(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(Self::CY, value);
    }

    pub fn ir(&self) -> bool {
        self.0.view_bits::<Lsb0>()[Self::IR]
    }

    pub fn set_ir(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(Self::IR, value);
    }

    pub fn hpm(&self, n: u8) -> bool {
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        self.0.view_bits::<Lsb0>()[n as usize]
    }

    pub fn set_hpm(&mut self, n: u8, value: bool) {
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        self.0.view_bits_mut::<Lsb0>().set(n as usize, value)
    }

    fn read(&self) -> u32 {
        self.0
    }

    fn write(&mut self, value: u32, mask: u32) {
        // Bit 1 is always read-only 0.
        self.0 = self.0 & !mask | value & mask & !0b10;
    }
}
