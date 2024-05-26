use bitvec::{field::BitField, order::Lsb0, view::BitView};

use super::{Exception, Interrupt};

#[derive(Debug, Clone)]
pub struct Trap {
    pub mtvec: Tvec,
    pub medeleg: Medeleg,
    pub mideleg: Mideleg,
    mscratch: u32,
    mepc: u32,
    pub mcause: Cause,
    mtval: u32,
    mtinst: u32,
    mtval2: u32,

    pub stvec: Tvec,
    sscratch: u32,
    sepc: u32,
    pub scause: Cause,
    stval: u32,
}

impl Default for Trap {
    fn default() -> Self {
        Self::new()
    }
}

impl Trap {
    pub fn new() -> Self {
        Self {
            mtvec: Tvec::new(),
            medeleg: Medeleg::new(),
            mideleg: Mideleg::new(),
            mscratch: 0,
            mepc: 0,
            mcause: Cause::new(),
            mtval: 0,
            mtinst: 0,
            mtval2: 0,

            stvec: Tvec::new(),
            sscratch: 0,
            sepc: 0,
            scause: Cause::new(),
            stval: 0,
        }
    }

    pub fn read_mscratch(&self) -> u32 {
        self.mscratch
    }

    pub fn write_mscratch(&mut self, value: u32, mask: u32) {
        self.mscratch = self.mscratch & !mask | value & mask;
    }

    pub fn read_mepc(&self) -> u32 {
        self.mepc
    }

    pub fn write_mepc(&mut self, value: u32, mask: u32) {
        self.mepc = self.mepc & !mask | value & mask & !0b11;
    }

    pub fn read_mtval(&self) -> u32 {
        self.mtval
    }

    pub fn write_mtval(&mut self, value: u32, mask: u32) {
        self.mtval = self.mtval & !mask | value & mask;
    }

    pub fn read_mtinst(&self) -> u32 {
        self.mtinst
    }

    pub fn write_mtinst(&mut self, value: u32, mask: u32) {
        self.mtinst = self.mtinst & !mask | value & mask;
    }

    pub fn read_mtval2(&self) -> u32 {
        self.mtval2
    }

    pub fn write_mtval2(&mut self, value: u32, mask: u32) {
        self.mtval2 = self.mtval2 & !mask | value & mask;
    }

    pub fn read_sscratch(&self) -> u32 {
        self.sscratch
    }

    pub fn write_sscratch(&mut self, value: u32, mask: u32) {
        self.sscratch = self.sscratch & !mask | value & mask;
    }

    pub fn read_sepc(&self) -> u32 {
        self.sepc
    }

    pub fn write_sepc(&mut self, value: u32, mask: u32) {
        self.sepc = self.sepc & !mask | value & mask;
        self.sepc &= !0b1;
    }

    pub fn read_stval(&self) -> u32 {
        self.stval
    }

    pub fn write_stval(&mut self, value: u32, mask: u32) {
        self.stval = self.stval & !mask | value & mask;
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
    #[allow(clippy::identity_op)] // To use this zero . here, which generates better formatting.
    const DELEGATABLE_EXCEPTIONS_MASK: u32 = 0  // <--'
        | (1 << Exception::INSTRUCTION_ADDRESS_MISALIGNED)
        | (1 << Exception::INSTRUCTION_ACCESS_FAULT)
        | (1 << Exception::ILLEGAL_INSTRUCTION)
        | (1 << Exception::BREAKPOINT)
        | (1 << Exception::LOAD_ADDRESS_MISALIGNED)
        | (1 << Exception::LOAD_ACCESS_FAULT)
        | (1 << Exception::STORE_OR_AMO_ADDRESS_MISALIGNED)
        | (1 << Exception::STORE_OR_AMO_ACCESS_FAULT)
        | (1 << Exception::ENVIRONMENT_CALL_FROM_U_MODE)
        | (1 << Exception::ENVIRONMENT_CALL_FROM_S_MODE)
        | (1 << Exception::ENVIRONMENT_CALL_FROM_M_MODE)
        | (1 << Exception::INSTRUCTION_PAGE_FAULT)
        | (1 << Exception::LOAD_PAGE_FAULT)
        | (1 << Exception::STORE_OR_AMO_PAGE_FAULT);

    pub fn new() -> Self {
        Self(0x0000_0000)
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

/// The mideleg register is **WARL**.
#[derive(Debug, Clone)]
pub struct Mideleg(u32);

impl Default for Mideleg {
    fn default() -> Self {
        Self::new()
    }
}

impl Mideleg {
    #![allow(unused)]

    // Delegetable interrupts according to QEMU's implementation.
    const DELEGATABLE_INTERRUPTS_MASK: u32 = (1 << Mip::SSIP) | (1 << Mip::STIP) | (1 << Mip::SEIP);

    pub fn new() -> Self {
        Self(0x0000_0000)
    }

    pub fn read(&self) -> u32 {
        self.0
    }

    pub fn write(&mut self, value: u32, mask: u32) {
        self.0 = self.0 & !mask | value & mask & Self::DELEGATABLE_INTERRUPTS_MASK;
    }

    pub fn should_delegate(&self, interrupt: Interrupt) -> bool {
        self.0 & (1 << interrupt.code()) != 0
    }
}

// Temporary placeholder for the real mip. TODO
#[allow(unused)]
#[derive(Debug, Clone)]
pub struct Mip(u16);

impl Mip {
    #![allow(unused)]

    // Bit indices for the fields of the mip register.
    const SSIP: usize = 1;
    // const MSIP: usize = 3;
    const STIP: usize = 5;
    // const MTIP: usize = 7;
    const SEIP: usize = 9;
    // const MEIP: usize = 11;
}

#[derive(Debug, Clone)]
pub struct Cause(u32);

impl Cause {
    pub fn new() -> Self {
        Self(0x0000_0000)
    }

    pub fn read(&self) -> u32 {
        self.0
    }

    pub fn write(&mut self, value: u32, mask: u32) {
        self.0 = self.0 & !mask | value & mask;
    }

    pub fn set(&mut self, cause: &TrapCause) {
        match cause {
            TrapCause::Exception(exception) => self.set_exception(Some(exception)),
            TrapCause::Interrupt(interrupt) => self.set_interrupt(Some(interrupt)),
        }
    }

    /// An `exception` of `None` indicates that the cause is unknown (results in all-zero code).
    pub fn set_exception(&mut self, exception: Option<&Exception>) {
        self.0 = exception.map(Exception::code).unwrap_or(0);
    }

    /// An `interrupt` of `None` indicates that the cause is unknown (results in all-zero code).
    pub fn set_interrupt(&mut self, interrupt: Option<&Interrupt>) {
        self.0 = 0x8000_0000 | interrupt.map(Interrupt::code).unwrap_or(0);
    }
}

#[derive(Debug, Clone)]
pub enum TrapCause {
    Exception(Exception),
    Interrupt(Interrupt),
}

impl From<Exception> for TrapCause {
    fn from(value: Exception) -> Self {
        Self::Exception(value)
    }
}

impl From<Interrupt> for TrapCause {
    fn from(value: Interrupt) -> Self {
        Self::Interrupt(value)
    }
}
