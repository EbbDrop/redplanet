#![allow(unused)]

use bitvec::{order::Lsb0, view::BitView};

#[derive(Debug, Clone)]
pub struct Control {
    pub mcounteren: Counteren,
    pub mcountinhibit: Mcountinhibit,

    pub scounteren: Counteren,
}

impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}

impl Control {
    pub fn new() -> Self {
        Self {
            mcounteren: Counteren::new(),
            mcountinhibit: Mcountinhibit::new(),
            scounteren: Counteren::new(),
        }
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
    // Bit indices for the fields of the counter-enable register.
    // Indicies 3 -> 31 map to HPM3 -> HPM31.
    const CY: usize = 0;
    const TM: usize = 1;
    const IR: usize = 2;

    pub fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub fn read(&self) -> u32 {
        self.0
    }

    pub fn write(&mut self, value: u32, mask: u32) {
        self.0 = self.0 & !mask | value & mask;
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
    // Bit indices for the fields of the mcountinhibit register.
    // Index 1 is a read-only zero bit.
    // Indicies 3 -> 31 map to HPM3 -> HPM31.
    const CY: usize = 0;
    const IR: usize = 2;

    pub fn new() -> Self {
        Self(0x0000_0000)
    }

    pub fn read(&self) -> u32 {
        self.0
    }

    pub fn write(&mut self, value: u32, mask: u32) {
        // Bit 1 is always read-only 0.
        self.0 = self.0 & !mask | value & mask & !0b10;
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
}
