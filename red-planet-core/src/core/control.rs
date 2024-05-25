use bitvec::{field::BitField, order::Lsb0, view::BitView};

use super::Exception;

#[derive(Debug, Clone)]
pub struct Control {
    pub mtvec: Tvec,
    pub medeleg: Medeleg,
    // TODO
    // pub mideleg: Mideleg,
    pub mcounteren: Counteren,
    pub mcountinhibit: Mcountinhibit,

    // TODO
    // pub sie: u32,
    pub stvec: Tvec,
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
            mtvec: Tvec::new(),
            medeleg: Medeleg::new(),
            // mideleg: todo!(),
            mcounteren: Counteren::new(),
            mcountinhibit: Mcountinhibit::new(),
            // sie: todo!(),
            stvec: Tvec::new(),
            scounteren: Counteren::new(),
        }
    }
}

/// Trap Vector Base Address Register (mtvec and stvec).
///
/// # mtvec
///
/// > The mtvec register is an MXLEN-bit WARL read/write register that holds trap vector
/// > configuration, consisting of a vector base address (BASE) and a vector mode (MODE).
///
/// > The mtvec register must always be implemented, but can contain a read-only value. If mtvec is
/// > writable, the set of values the register may hold can vary by implementation. The value in the
/// > BASE field must always be aligned on a 4-byte boundary, and the MODE setting may impose
/// > additional alignment constraints on the value in the BASE field.
///
/// > When MODE=Direct, all traps into machine mode cause the pc to be set to the address in the
/// > BASE field. When MODE=Vectored, all synchronous exceptions into machine mode cause the pc to
/// > be set to the address in the BASE field, whereas interrupts cause the pc to be set to the
/// > address in the BASE field plus four times the interrupt cause number. For example, a
/// > machine-mode timer interrupt [...] causes the pc to be set to BASE+0x1c.
///
/// > An implementation may have different alignment constraints for different modes. In particular,
/// > MODE=Vectored may have stricter alignment constraints than MODE=Direct.
///
/// # stvec
///
/// > The stvec register is an SXLEN-bit read/write register that holds trap vector configuration,
/// > consisting of a vector base address (BASE) and a vector mode (MODE).
///
/// > The BASE field in stvec is a WARL field that can hold any valid virtual or physical address,
/// > subject to the following alignment constraints: the address must be 4-byte aligned, and MODE
/// > settings other than Direct might impose additional alignment constraints on the value in the
/// > BASE field.
///
/// > The encoding of the MODE field is shown in Table 4.1. When MODE=Direct, all traps into
/// > supervisor mode cause the pc to be set to the address in the BASE field. When MODE=Vectored,
/// > all synchronous exceptions into supervisor mode cause the pc to be set to the address in the
/// > BASE field, whereas interrupts cause the pc to be set to the address in the BASE field plus
/// > four times the interrupt cause number. For example, a supervisor-mode timer interrupt [...]
/// > causes the pc to be set to BASE+0x14. Setting MODE=Vectored may impose a stricter alignment
/// > constraint on BASE.
#[derive(Debug, Clone)]
pub struct Tvec(u32);

impl Default for Tvec {
    fn default() -> Self {
        Self::new()
    }
}

impl Tvec {
    pub fn new() -> Self {
        Self(0x0000_0000)
    }

    pub fn read(&self) -> u32 {
        self.0
    }

    pub fn write(&mut self, value: u32, mask: u32) {
        // Ignored, since this is imlemented as a read-only register.
        let new_value = self.0 & !mask | value & mask;
        if new_value & 0b11 >= 2 {
            // Reserved MODE.
            // Since this is a WARL register, we can set the register to any legal value here.
            // Choose to preserve the old value, matching the behavior of QEMU's implementation.
        } else {
            self.0 = new_value;
        }
    }

    /// Returns the vector base address (stored in BASE field).
    ///
    /// Note that the returned address was encoded in the field right shifted by 2 bits.
    pub fn base(&self) -> u32 {
        self.0.view_bits::<Lsb0>()[2..].load_le::<u32>() << 2
    }

    /// Returns the vector mode (stored in MODE field).
    pub fn mode(&self) -> VectorMode {
        // The 2 least significant bits of self.0 encode the vector mode.
        // Since values >= 2 are reserved, only values 0 and 1 are possible.
        match self.0.view_bits::<Lsb0>()[0] {
            false => VectorMode::Direct,
            true => VectorMode::Vectored,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorMode {
    Direct,
    Vectored,
}

/// The medeleg register is **WARL**.
#[derive(Debug, Clone)]
pub struct Medeleg(u32);

impl Default for Medeleg {
    fn default() -> Self {
        Self::new()
    }
}

impl Medeleg {
    // Delegetable exceptions according to QEMU's implementation.
    #[allow(clippy::identity_op)] // To use this zero , here, which generates better formatting.
    const DELEGATABLE_EXCEPTIONS_MASK: u32 = (0  // <-*
        | (1 << Exception::InstructionAddressMisaligned.code())
        | (1 << Exception::InstructionAccessFault.code())
        | (1 << Exception::IllegalInstruction.code())
        | (1 << Exception::Breakpoint.code())
        | (1 << Exception::LoadAddressMisaligned.code())
        | (1 << Exception::LoadAccessFault.code())
        | (1 << Exception::StoreOrAmoAddressMisaligned.code())
        | (1 << Exception::StoreOrAmoAccessFault.code())
        | (1 << Exception::EnvironmentCallFromUMode.code())
        | (1 << Exception::EnvironmentCallFromSMode.code())
        | (1 << Exception::EnvironmentCallFromMMode.code())
        | (1 << Exception::InstructionPageFault.code())
        | (1 << Exception::LoadPageFault.code())
        | (1 << Exception::StoreOrAmoPageFault.code()));

    pub fn new() -> Self {
        Self(0)
    }

    pub fn read(&self) -> u32 {
        self.0
    }

    pub fn write(&mut self, value: u32, mask: u32) {
        self.0 = self.0 & !mask | value & mask & Self::DELEGATABLE_EXCEPTIONS_MASK;
    }

    pub fn should_delegate(&self, exception: Exception) -> bool {
        self.0 & (1 << exception.code()) != 0
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
