use bitvec::{order::Lsb0, view::BitView};

#[derive(Debug, Clone)]
pub struct Mconfig {
    menvcfg: u32,
    menvcfgh: u32,
}

impl Default for Mconfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Mconfig {
    pub fn new() -> Self {
        Self {
            menvcfg: 0x0000_0000,
            menvcfgh: 0x0000_0000,
        }
    }

    pub fn read_menvcfg(&self) -> u32 {
        self.menvcfg
    }

    pub fn write_menvcfg(&mut self, value: u32, mask: u32) {
        self.menvcfg = self.menvcfg & !mask | value & mask;
    }

    pub fn read_menvcfgh(&self) -> u32 {
        self.menvcfgh
    }

    pub fn write_menvcfgh(&mut self, value: u32, mask: u32) {
        self.menvcfgh = self.menvcfgh & !mask | value & mask;
    }

    pub fn fiom(&self) -> bool {
        self.menvcfg.view_bits::<Lsb0>()[idx::FIOM]
    }

    pub fn set_fiom(&mut self, value: bool) {
        self.menvcfg.view_bits_mut::<Lsb0>().set(idx::FIOM, value);
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
