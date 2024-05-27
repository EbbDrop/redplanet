//! Trap-related state and read/write logic for corresponding CSRs on [`Core`].

use bitvec::{array::BitArray, field::BitField, order::Lsb0, view::BitView};
use space_time::allocator::Allocator;

use crate::system_bus::SystemBus;

use super::{Core, Exception, ExceptionCode, Interrupt};

// Delegetable exceptions according to QEMU's implementation.
#[allow(clippy::identity_op)] // To use this zero . here, which generates better formatting.
const DELEGATABLE_EXCEPTIONS_MASK: u32 = 0  // <--'
        | (1 << ExceptionCode::INSTRUCTION_ADDRESS_MISALIGNED)
        | (1 << ExceptionCode::INSTRUCTION_ACCESS_FAULT)
        | (1 << ExceptionCode::ILLEGAL_INSTRUCTION)
        | (1 << ExceptionCode::BREAKPOINT)
        | (1 << ExceptionCode::LOAD_ADDRESS_MISALIGNED)
        | (1 << ExceptionCode::LOAD_ACCESS_FAULT)
        | (1 << ExceptionCode::STORE_OR_AMO_ADDRESS_MISALIGNED)
        | (1 << ExceptionCode::STORE_OR_AMO_ACCESS_FAULT)
        | (1 << ExceptionCode::ENVIRONMENT_CALL_FROM_U_MODE)
        | (1 << ExceptionCode::ENVIRONMENT_CALL_FROM_S_MODE)
        | (1 << ExceptionCode::ENVIRONMENT_CALL_FROM_M_MODE)
        | (1 << ExceptionCode::INSTRUCTION_PAGE_FAULT)
        | (1 << ExceptionCode::LOAD_PAGE_FAULT)
        | (1 << ExceptionCode::STORE_OR_AMO_PAGE_FAULT);

// Delegetable interrupts according to QEMU's implementation.
const DELEGATABLE_INTERRUPTS_MASK: u32 = (1 << Mip::SSIP) | (1 << Mip::STIP) | (1 << Mip::SEIP);

/// Stores trap-related state. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Trap {
    /// M-mode vector base address. Always word-aligned.
    m_vector_base_address: u32,
    /// M-mode vector mode (direct or vectored).
    m_vector_mode: VectorMode,
    /// S-mode vector base address. Always word-aligned.
    s_vector_base_address: u32,
    /// S-mode vector mode (direct or vectored).
    s_vector_mode: VectorMode,

    /// M-mode scratch register.
    mscratch: u32,
    /// S-mode scratch register.
    sscratch: u32,

    /// M-mode Exception Program Counter. Always word-aligned.
    mepc: u32,
    /// S-mode Exception Program Counter. Always word-aligned.
    sepc: u32,

    /// Last M-mode trap cause (interrupt or exception).
    /// Initialized to unknown exception, which corresponds to an all-zero mcause value.
    last_m_trap_cause: Cause,
    /// Code may write legal values to the mcause register, replacing the value from the last trap.
    /// This will be `Some` if such override happened, or `None` otherwise.
    mcause_override: Option<CauseCode>,
    /// Last S-mode trap cause (interrupt or exception).
    /// Initialized to unknown exception, which corresponds to an all-zero scause value.
    last_s_trap_cause: Cause,
    /// Code may write legal values to the scause register, replacing the value from the last trap.
    /// This will be `Some` if such override happened, or `None` otherwise.
    scause_override: Option<CauseCode>,

    /// Array of booleans, with for each bit index matching an exception's code a bool indicating
    /// whether handling that exception should be delegated to S-mode (if not caused from M-mode).
    delegate_exception: BitArray<[u32; 1], Lsb0>,
    /// Array of booleans, with for each bit index matching an interrupts's code a bool indicating
    /// whether handling that interrupt should be delegated to S-mode (if not triggered in M-mode).
    delegate_interrupt: BitArray<[u32; 1], Lsb0>,

    /// The value associated with a trap handled in M-mode, or zero if there is no such data.
    mtval: u32,
    /// Optional second value associated with a trap handled in M-mode, or zero if there is nosuch data.
    mtval2: u32,
    /// Optional value providing additional information on the instruction that trapped, if the
    /// trap is handled in M-mode, or zero if there is no such data.
    mtinst: u32,
    /// The value associated with an exception/interrupt handled in S-mode, or zero if there is no
    /// such data.
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
            m_vector_base_address: 0x0000_0000,
            m_vector_mode: VectorMode::Direct,
            s_vector_base_address: 0x0000_0000,
            s_vector_mode: VectorMode::Direct,
            mscratch: 0x0000_0000,
            sscratch: 0x0000_0000,
            mepc: 0x0000_0000,
            sepc: 0x0000_0000,
            last_m_trap_cause: Cause::Exception(None),
            mcause_override: None,
            last_s_trap_cause: Cause::Exception(None),
            scause_override: None,
            // TODO: Should this default to 0xFFFF_FFFF?
            delegate_exception: BitArray::new([0x0000_0000]),
            delegate_interrupt: BitArray::new([0x0000_0000]),
            mtval: 0x0000_0000,
            mtval2: 0x0000_0000,
            mtinst: 0x0000_0000,
            stval: 0x0000_0000,
        }
    }

    /// M-mode vector base address. Always word-aligned.
    pub fn m_vector_base_address(&self) -> u32 {
        self.m_vector_base_address
    }

    /// M-mode vector mode (direct or vectored).
    pub fn m_vector_mode(&self) -> VectorMode {
        self.m_vector_mode
    }

    /// S-mode vector base address. Always word-aligned.
    pub fn s_vector_base_address(&self) -> u32 {
        self.s_vector_base_address
    }

    /// S-mode vector mode (direct or vectored).
    pub fn s_vector_mode(&self) -> VectorMode {
        self.s_vector_mode
    }

    /// Returns the word-aligned M-mode Exception Program Counter.
    ///
    /// Note that this returns the value of the mepc register, which may have been written to the
    /// by guest code since the last call to [`Self::set_mepc`].
    pub fn mepc(&self) -> u32 {
        self.mepc
    }

    /// Sets the M-mode Exception Program Counter to the word-aligned `address`.
    ///
    /// # Panics
    ///
    /// Panics of `address` is not word-aligned.
    pub fn set_mepc(&mut self, address: u32) {
        assert!(address & 0b11 == 0);
        self.mepc = address;
    }

    /// Returns the word-aligned S-mode Exception Program Counter.
    ///
    /// Note that this returns the value of the sepc register, which may have been written to the
    /// by guest code since the last call to [`Self::set_sepc`].
    pub fn sepc(&self) -> u32 {
        self.sepc
    }

    /// Sets the S-mode Exception Program Counter to the word-aligned `address`.
    ///
    /// # Panics
    ///
    /// Panics of `address` is not word-aligned.
    pub fn set_sepc(&mut self, address: u32) {
        assert!(address & 0b11 == 0);
        self.sepc = address;
    }

    /// Last M-mode trap cause (interrupt or exception).
    ///
    /// This is not necessarily the value a CSR read of mcause would return! This is because code
    /// may have written a new (legal) value to mcause since the last M-mode exception. This method
    /// will always return the last cause set with [`Self::set_m_trap_cause`].
    ///
    /// Initially, if no exceptions have occurred yet, this will return `Cause::Exception(None)`.
    #[allow(dead_code)] // TODO
    pub fn last_m_trap_cause(&self) -> &Cause {
        &self.last_m_trap_cause
    }

    /// Last S-mode trap cause (interrupt or exception).
    ///
    /// This is not necessarily the value a CSR read of scause would return! This is because code
    /// may have written a new (legal) value to mcause since the last S-mode exception. This method
    /// will always return the last cause set with [`Self::set_s_trap_cause`].
    ///
    /// Initially, if no exceptions have occurred yet, this will return `Cause::Exception(None)`.
    #[allow(dead_code)] // TODO
    pub fn last_s_trap_cause(&self) -> &Cause {
        &self.last_s_trap_cause
    }

    /// Indicate a trap caused by `cause` is taken in M-mode.
    ///
    /// If the cause is unknown, an exception/interrupt of `None` can be used.
    ///
    /// See also [`Self::last_m_trap_cause`].
    pub fn set_m_trap_cause(&mut self, cause: impl Into<Cause>) {
        self.last_m_trap_cause = cause.into();
        self.mcause_override = None;
    }

    /// Indicate a trap caused by `cause` is taken in S-mode.
    ///
    /// If the cause is unknown, an exception/interrupt of `None` can be used.
    ///
    /// See also [`Self::last_s_trap_cause`].
    pub fn set_s_trap_cause(&mut self, cause: impl Into<Cause>) {
        self.last_s_trap_cause = cause.into();
        self.scause_override = None;
    }

    /// Returns `true` if the medeleg/mideleg register indicates a trap caused by `cause` should be
    /// delegated to S-mode.
    ///
    /// Note that traps triggered in M-mode should always be handled in M-mode, even if this method
    /// returns `true`.
    pub fn should_delegate(&self, cause: impl Into<CauseCode>) -> bool {
        match cause.into() {
            CauseCode::Exception(Some(exception)) => match exception as u8 {
                code @ 0..=31 => self.delegate_exception[code as usize],
                _ => false,
            },
            CauseCode::Interrupt(Some(interrupt)) => match interrupt.code() {
                code @ 0..=31 => self.delegate_interrupt[code as usize],
                _ => false,
            },
            _ => false,
        }
    }

    /// Sets the value of the mtval register to `value`.
    // TODO: Enforce appropriate restrictions.
    pub fn set_mtval(&mut self, value: u32) {
        self.mtval = value;
    }

    /// Sets the value of the mtval2 register to `value`.
    // TODO: Enforce appropriate restrictions.
    pub fn set_mtval2(&mut self, value: u32) {
        self.mtval2 = value;
    }

    /// Sets the value of the mtinst register to `value`.
    // TODO: Enforce appropriate restrictions.
    pub fn set_mtinst(&mut self, value: u32) {
        self.mtinst = value;
    }

    /// Sets the value of the stval register to `value`.
    // TODO: Enforce appropriate restrictions.
    pub fn set_stval(&mut self, value: u32) {
        self.stval = value;
    }
}

// Implementation of CSR read/write methods.
impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    /// Read mtvec register.
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
    pub fn read_mtvec(&self, allocator: &mut A) -> u32 {
        let trap = self.trap.get(allocator);
        read_tvec(trap.m_vector_base_address, trap.m_vector_mode)
    }

    /// Write mtvec register. See [`Self::read_mtvec`].
    pub fn write_mtvec(&self, allocator: &mut A, value: u32, mask: u32) {
        let trap = self.trap.get_mut(allocator);
        write_tvec(
            &mut trap.m_vector_base_address,
            &mut trap.m_vector_mode,
            value,
            mask,
        );
    }

    /// Read stvec register.
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
    pub fn read_stvec(&self, allocator: &mut A) -> u32 {
        let trap = self.trap.get(allocator);
        read_tvec(trap.s_vector_base_address, trap.s_vector_mode)
    }

    /// Write stvec register. See [`Self::read_stvec`].
    pub fn write_stvec(&self, allocator: &mut A, value: u32, mask: u32) {
        let trap = self.trap.get_mut(allocator);
        write_tvec(
            &mut trap.s_vector_base_address,
            &mut trap.s_vector_mode,
            value,
            mask,
        );
    }

    pub fn read_mscratch(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).mscratch
    }

    pub fn write_mscratch(&self, allocator: &mut A, value: u32, mask: u32) {
        let mscratch = &mut self.trap.get_mut(allocator).mscratch;
        *mscratch = *mscratch & !mask | value & mask;
    }

    pub fn read_sscratch(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).sscratch
    }

    pub fn write_sscratch(&self, allocator: &mut A, value: u32, mask: u32) {
        let sscratch = &mut self.trap.get_mut(allocator).sscratch;
        *sscratch = *sscratch & !mask | value & mask;
    }

    pub fn read_mepc(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).mepc
    }

    pub fn write_mepc(&self, allocator: &mut A, value: u32, mask: u32) {
        let mepc = &mut self.trap.get_mut(allocator).mepc;
        *mepc = *mepc & !mask | value & mask & !0b11;
    }

    pub fn read_sepc(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).sepc
    }

    pub fn write_sepc(&self, allocator: &mut A, value: u32, mask: u32) {
        let sepc = &mut self.trap.get_mut(allocator).sepc;
        *sepc = *sepc & !mask | value & mask & !0b11;
    }

    pub fn read_mcause(&self, allocator: &mut A) -> u32 {
        let trap = self.trap.get(allocator);
        read_cause(&trap.last_m_trap_cause, &trap.mcause_override)
    }

    pub fn write_mcause(&self, allocator: &mut A, value: u32, mask: u32) {
        let trap = self.trap.get_mut(allocator);
        write_cause(
            &trap.last_m_trap_cause,
            &mut trap.mcause_override,
            value,
            mask,
        );
    }

    pub fn read_scause(&self, allocator: &mut A) -> u32 {
        let trap = self.trap.get(allocator);
        read_cause(&trap.last_m_trap_cause, &trap.scause_override)
    }

    pub fn write_scause(&self, allocator: &mut A, value: u32, mask: u32) {
        let trap = self.trap.get_mut(allocator);
        write_cause(
            &trap.last_s_trap_cause,
            &mut trap.scause_override,
            value,
            mask,
        );
    }

    pub fn read_medeleg(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).delegate_exception.load_le()
    }

    /// The medeleg register is **WARL**.
    pub fn write_medeleg(&self, allocator: &mut A, value: u32, mask: u32) {
        let medeleg = &mut self.trap.get_mut(allocator).delegate_exception;
        let old_value = medeleg.load_le::<u32>();
        medeleg.store_le(old_value & !mask | value & mask & DELEGATABLE_EXCEPTIONS_MASK);
    }

    pub fn read_mideleg(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).delegate_interrupt.load_le()
    }

    /// The mideleg register is **WARL**.
    pub fn write_mideleg(&self, allocator: &mut A, value: u32, mask: u32) {
        let mideleg = &mut self.trap.get_mut(allocator).delegate_interrupt;
        let old_value = mideleg.load_le::<u32>();
        mideleg.store_le(old_value & !mask | value & mask & DELEGATABLE_INTERRUPTS_MASK);
    }

    pub fn read_mtval(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).mtval
    }

    pub fn write_mtval(&self, allocator: &mut A, value: u32, mask: u32) {
        let mtval = &mut self.trap.get_mut(allocator).mtval;
        *mtval = *mtval & !mask | value & mask;
    }

    pub fn read_mtval2(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).mtval2
    }

    pub fn write_mtval2(&self, allocator: &mut A, value: u32, mask: u32) {
        let mtval2 = &mut self.trap.get_mut(allocator).mtval2;
        *mtval2 = *mtval2 & !mask | value & mask;
    }

    pub fn read_mtinst(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).mtinst
    }

    pub fn write_mtinst(&self, allocator: &mut A, value: u32, mask: u32) {
        let mtinst = &mut self.trap.get_mut(allocator).mtinst;
        *mtinst = *mtinst & !mask | value & mask;
    }

    pub fn read_stval(&self, allocator: &mut A) -> u32 {
        self.trap.get(allocator).stval
    }

    pub fn write_stval(&self, allocator: &mut A, value: u32, mask: u32) {
        let stval = &mut self.trap.get_mut(allocator).stval;
        *stval = *stval & !mask | value & mask;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorMode {
    Direct,
    Vectored,
}

#[derive(Debug, Clone)]
pub enum Cause {
    /// Trap was caused by an exception.
    /// An inner value of `None` indicates the exception cause is unknown, e.g. after a Reset.
    Exception(Option<Exception>),
    /// Trap was caused by an interrupt.
    /// An inner value of `None` indicates the interrupt cause is unknown, e.g. when a NMI occurs.
    Interrupt(Option<Interrupt>),
}

impl Cause {
    pub fn code(&self) -> CauseCode {
        match self {
            Cause::Exception(exception) => exception.as_ref().map(|e| e.code()).into(),
            Cause::Interrupt(interrupt) => (*interrupt).into(),
        }
    }
}

impl From<Exception> for Cause {
    fn from(value: Exception) -> Self {
        Self::Exception(Some(value))
    }
}

impl From<Option<Exception>> for Cause {
    fn from(value: Option<Exception>) -> Self {
        Self::Exception(value)
    }
}

impl From<Interrupt> for Cause {
    fn from(value: Interrupt) -> Self {
        Self::Interrupt(Some(value))
    }
}

impl From<Option<Interrupt>> for Cause {
    fn from(value: Option<Interrupt>) -> Self {
        Self::Interrupt(value)
    }
}

#[derive(Debug, Clone)]
pub enum CauseCode {
    Exception(Option<ExceptionCode>),
    Interrupt(Option<Interrupt>),
}

impl From<ExceptionCode> for CauseCode {
    fn from(value: ExceptionCode) -> Self {
        Self::Exception(Some(value))
    }
}

impl From<Option<ExceptionCode>> for CauseCode {
    fn from(value: Option<ExceptionCode>) -> Self {
        Self::Exception(value)
    }
}

impl From<Interrupt> for CauseCode {
    fn from(value: Interrupt) -> Self {
        Self::Interrupt(Some(value))
    }
}

impl From<Option<Interrupt>> for CauseCode {
    fn from(value: Option<Interrupt>) -> Self {
        Self::Interrupt(value)
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

fn read_tvec(base_address: u32, mode: VectorMode) -> u32 {
    assert!(base_address & 0b11 == 0);
    let mode_bits = match mode {
        VectorMode::Direct => 0b01,
        VectorMode::Vectored => 0b10,
    };
    base_address | mode_bits
}

fn write_tvec(base_address: &mut u32, mode: &mut VectorMode, value: u32, mask: u32) {
    let tvec = read_tvec(*base_address, *mode) & !mask | value & mask;
    *mode = match tvec & 0b11 {
        0 => VectorMode::Direct,
        1 => VectorMode::Vectored,
        _ => {
            // Reserved MODE.
            // Since this is a WARL register, we can set the register to any legal value here.
            // Choose to preserve the old value, matching the behavior of QEMU's implementation.
            return;
        }
    };
    *base_address = tvec & !0b11;
}

fn read_cause(last_trap_cause: &Cause, cause_override: &Option<CauseCode>) -> u32 {
    // If the register has been overwritten, return the override.
    if let Some(code) = cause_override {
        return match code {
            CauseCode::Exception(exc_code) => exc_code.map_or(0, |c| c as u32),
            CauseCode::Interrupt(interrupt) => 0x8000_0000 | interrupt.map_or(0, |i| i.code()),
        };
    }
    // Otherwise, return the value dervived from the last trap's cause.
    match last_trap_cause {
        Cause::Exception(exception) => exception.map_or(0, |e| e.code() as u32),
        Cause::Interrupt(interrupt) => 0x8000_0000 | interrupt.map_or(0, |i| i.code()),
    }
}

fn write_cause(
    last_trap_cause: &Cause,
    cause_override: &mut Option<CauseCode>,
    value: u32,
    mask: u32,
) {
    let mut cause = read_cause(last_trap_cause, cause_override) & !mask | value & mask;
    let is_interrupt = cause.view_bits_mut::<Lsb0>().replace(31, false);
    *cause_override = Some(match is_interrupt {
        false if cause == 0 => CauseCode::Exception(None),
        false => match ExceptionCode::try_from(cause) {
            Ok(code) => CauseCode::Exception(Some(code)),
            Err(_) => return,
        },
        true if cause == 0 => CauseCode::Interrupt(None),
        true => match Interrupt::try_from(cause) {
            Ok(code) => CauseCode::Interrupt(Some(code)),
            Err(_) => return,
        },
    });
}
