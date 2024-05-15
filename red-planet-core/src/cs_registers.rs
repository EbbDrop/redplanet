//! Allocated Control and Status Registers.
//!
//! Part of the "Zicsr" extension.

use crate::{Allocated, Allocator, PrivilegeLevel, RawPrivilegeLevel};
use thiserror::Error;

/// Control and Status Registers for a single RV32I hart.
///
/// Note that no access control is provided, i.e. all registers can be accessed independently of the
/// configured privilege level.
///
/// > RISC-V defines a separate address space of 4096 Control and Status registers associated with
/// > each hart.
///
/// > The standard RISC-V ISA sets aside a 12-bit encoding space (csr\[11:0]) for up to 4,096 CSRs.
/// > By convention, the upper 4 bits of the CSR address (csr\[11:8]) are used to encode the read
/// > and write accessibility of the CSRs according to privilege level as shown in Table 2.1. The
/// > top two bits (csr\[11:10]) indicate whether the register is read/write (00, 01, or 10) or
/// > read-only (11). The next two bits (csr\[9:8]) encode the lowest privilege level that can
/// > access the CSR.
#[derive(Debug)]
pub struct CSRegisters<A: Allocator> {
    /// Index in the allocator where all CSR counter registers are stored.
    ///
    /// These are allocated together, since at least a subset of them will be updated every tick,
    /// and most likely more will be updated in between snapshots.
    ///
    /// > RISC-V ISAs provide a set of up to 32×64-bit performance counters and timers that are
    /// > accessible via unprivileged XLEN read-only CSR registers 0xC00–0xC1F (with the upper 32
    /// > bits accessed via CSR registers 0xC80–0xC9F on RV32). The first three of these (CYCLE,
    /// > TIME, and INSTRET) have dedicated functions (cycle count, real-time clock, and
    /// > instructions-retired respectively), while the remaining counters, if implemented, provide
    /// > programmable event counting.
    counters: Allocated<A, [u64; 32]>,
}

impl<A: Allocator> CSRegisters<A> {
    /// Creates a fresh collection of registers initialized to their reset values.
    pub fn new(allocator: &mut A) -> Self {
        Self {
            counters: Allocated::new(allocator, [0; 32]),
        }
    }

    /// Force all Control and Status registers to their reset state.
    pub fn reset(&self, allocator: &mut A) {
        *self.counters.get_mut(allocator) = [0; 32];
    }

    /// Read the value of a CSR by its specifier.
    ///
    /// `privilege_level` indicates at what privilege level the read is performed. If the CSR that
    /// is being read requires a higher privilege level (see
    /// [`CsrSpecifier::required_privilege_level`]), then an [`AccessError::Privileged`] will be given.
    pub fn read(
        &self,
        allocator: &mut A,
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
    ) -> Result<u32, AccessError> {
        Self::check_access(specifier, privilege_level)?;
        match specifier {
            specifier::CYCLE
            | specifier::TIME
            | specifier::INSTRET
            | specifier::HPMCOUNTER3..=specifier::HPMCOUNTER31 => {
                let offset = specifier as usize - specifier::CYCLE as usize;
                Ok(self.counters.get(allocator)[offset] as u32)
            }
            specifier::CYCLEH
            | specifier::TIMEH
            | specifier::INSTRETH
            | specifier::HPMCOUNTER3H..=specifier::HPMCOUNTER31H => {
                let offset = specifier as usize - specifier::CYCLEH as usize;
                Ok((self.counters.get(allocator)[offset] >> 32) as u32)
            }
            _ => todo!(),
        }
    }

    pub fn write(
        &self,
        _allocator: &mut A,
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
        _value: u32,
        _mask: u32,
    ) -> Result<(), WriteError> {
        Self::check_access(specifier, privilege_level).map_err(WriteError::AccessError)?;
        if specifier::is_read_only(specifier) {
            return Err(WriteError::WriteToReadOnly);
        }
        todo!()
    }

    fn check_access(
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
    ) -> Result<(), AccessError> {
        if !specifier::is_valid(specifier) {
            return Err(AccessError::CsrUnsupported(specifier));
        }
        let required_level = specifier::required_privilege_level(specifier);
        if privilege_level < required_level {
            return Err(AccessError::Privileged {
                specifier,
                required_level,
                actual_level: privilege_level,
            });
        }
        Ok(())
    }
}

/// Errors that can occur when attempting to access a CSR.
#[derive(Error, Debug)]
pub enum AccessError {
    #[error("unsupported CSR: {0:#05X}")]
    CsrUnsupported(CsrSpecifier),
    /// Attempt to access a CSR that requires a higher privilege level.
    #[error(
        "cannot access specifier {specifier:#05X} from privilege level {actual_level}, \
             since it requires privilege level {required_level}"
    )]
    Privileged {
        /// The CSR for which access was requested.
        specifier: CsrSpecifier,
        /// The minimum required privilege level to access that CSR.
        required_level: RawPrivilegeLevel,
        /// The actual privilegel level from which the access was performed.
        actual_level: PrivilegeLevel,
    },
}

/// Errors that can occur when attempting to write to a CSR.
#[derive(Error, Debug)]
pub enum WriteError {
    /// A non-write specific access error. See [`AccessError`].
    #[error("{0}")]
    AccessError(AccessError),
    /// Attempt to write to a read-only register.
    #[error("writing to read-only CSR is invalid")]
    WriteToReadOnly,
}

/// General 12-bit value representing a CSR specifier. Note that this can hold any 12-bit value,
/// even if the value represents an unsupported or non-existent CSR.
pub type CsrSpecifier = u16;

/// Specifiers for all supported CSRs.
/// Debug-mode CSRs are not supported.
/// The hypervisor extension is also not supported.
pub mod specifier {
    use crate::RawPrivilegeLevel;

    use super::CsrSpecifier;

    //
    // Unprivileged floating-point CSRs (`0x001..=0x003`).
    //
    /// Floating-point accrued exceptions.
    pub const FFLAGS: CsrSpecifier = 0x001;
    /// Floating-point dynamic rounding mode.
    pub const FRM: CsrSpecifier = 0x002;
    /// Floating-point CSR ([`Self::Frm`] + [`Self::Fflags`]).
    pub const FCSR: CsrSpecifier = 0x003;

    //
    // Unprivileged counters/timers (`0xC00..=0xC1F`, `0xC80..=0xC9F`).
    //
    /// Cycle counter for RDCYCLE instruction.
    pub const CYCLE: CsrSpecifier = 0xC00;
    /// Timer for RDTIME instruction.
    pub const TIME: CsrSpecifier = 0xC01;
    /// Instructions-retired counter for RDINSTRET instruction.
    pub const INSTRET: CsrSpecifier = 0xC02;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER3: CsrSpecifier = 0xC03;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER4: CsrSpecifier = 0xC04;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER5: CsrSpecifier = 0xC05;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER6: CsrSpecifier = 0xC06;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER7: CsrSpecifier = 0xC07;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER8: CsrSpecifier = 0xC08;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER9: CsrSpecifier = 0xC09;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER10: CsrSpecifier = 0xC0A;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER11: CsrSpecifier = 0xC0B;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER12: CsrSpecifier = 0xC0C;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER13: CsrSpecifier = 0xC0D;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER14: CsrSpecifier = 0xC0E;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER15: CsrSpecifier = 0xC0F;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER16: CsrSpecifier = 0xC10;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER17: CsrSpecifier = 0xC11;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER18: CsrSpecifier = 0xC12;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER19: CsrSpecifier = 0xC13;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER20: CsrSpecifier = 0xC14;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER21: CsrSpecifier = 0xC15;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER22: CsrSpecifier = 0xC16;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER23: CsrSpecifier = 0xC17;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER24: CsrSpecifier = 0xC18;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER25: CsrSpecifier = 0xC19;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER26: CsrSpecifier = 0xC1A;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER27: CsrSpecifier = 0xC1B;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER28: CsrSpecifier = 0xC1C;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER29: CsrSpecifier = 0xC1D;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER30: CsrSpecifier = 0xC1E;
    /// Performance-monitoring counter.
    pub const HPMCOUNTER31: CsrSpecifier = 0xC1F;
    // RV32-only registers for the upper 32 bits of all counter registers.
    /// Upper 32 bits of [`Self::Cycle`], RV32 only.
    pub const CYCLEH: CsrSpecifier = 0xC80;
    /// Upper 32 bits of [`Self::Time`], RV32 only.
    pub const TIMEH: CsrSpecifier = 0xC81;
    /// Upper 32 bits of [`Self::Instret`], RV32 only.
    pub const INSTRETH: CsrSpecifier = 0xC82;
    /// Upper 32 bits of [`Self::Hpmcounter3`], RV32 only.
    pub const HPMCOUNTER3H: CsrSpecifier = 0xC83;
    /// Upper 32 bits of [`Self::Hpmcounter4`], RV32 only.
    pub const HPMCOUNTER4H: CsrSpecifier = 0xC84;
    /// Upper 32 bits of [`Self::Hpmcounter5`], RV32 only.
    pub const HPMCOUNTER5H: CsrSpecifier = 0xC85;
    /// Upper 32 bits of [`Self::Hpmcounter6`], RV32 only.
    pub const HPMCOUNTER6H: CsrSpecifier = 0xC86;
    /// Upper 32 bits of [`Self::Hpmcounter7`], RV32 only.
    pub const HPMCOUNTER7H: CsrSpecifier = 0xC87;
    /// Upper 32 bits of [`Self::Hpmcounter8`], RV32 only.
    pub const HPMCOUNTER8H: CsrSpecifier = 0xC88;
    /// Upper 32 bits of [`Self::Hpmcounter9`], RV32 only.
    pub const HPMCOUNTER9H: CsrSpecifier = 0xC89;
    /// Upper 32 bits of [`Self::Hpmcounter10`], RV32 only.
    pub const HPMCOUNTER10H: CsrSpecifier = 0xC8A;
    /// Upper 32 bits of [`Self::Hpmcounter11`], RV32 only.
    pub const HPMCOUNTER11H: CsrSpecifier = 0xC8B;
    /// Upper 32 bits of [`Self::Hpmcounter12`], RV32 only.
    pub const HPMCOUNTER12H: CsrSpecifier = 0xC8C;
    /// Upper 32 bits of [`Self::Hpmcounter13`], RV32 only.
    pub const HPMCOUNTER13H: CsrSpecifier = 0xC8D;
    /// Upper 32 bits of [`Self::Hpmcounter14`], RV32 only.
    pub const HPMCOUNTER14H: CsrSpecifier = 0xC8E;
    /// Upper 32 bits of [`Self::Hpmcounter15`], RV32 only.
    pub const HPMCOUNTER15H: CsrSpecifier = 0xC8F;
    /// Upper 32 bits of [`Self::Hpmcounter16`], RV32 only.
    pub const HPMCOUNTER16H: CsrSpecifier = 0xC90;
    /// Upper 32 bits of [`Self::Hpmcounter17`], RV32 only.
    pub const HPMCOUNTER17H: CsrSpecifier = 0xC91;
    /// Upper 32 bits of [`Self::Hpmcounter18`], RV32 only.
    pub const HPMCOUNTER18H: CsrSpecifier = 0xC92;
    /// Upper 32 bits of [`Self::Hpmcounter19`], RV32 only.
    pub const HPMCOUNTER19H: CsrSpecifier = 0xC93;
    /// Upper 32 bits of [`Self::Hpmcounter20`], RV32 only.
    pub const HPMCOUNTER20H: CsrSpecifier = 0xC94;
    /// Upper 32 bits of [`Self::Hpmcounter21`], RV32 only.
    pub const HPMCOUNTER21H: CsrSpecifier = 0xC95;
    /// Upper 32 bits of [`Self::Hpmcounter22`], RV32 only.
    pub const HPMCOUNTER22H: CsrSpecifier = 0xC96;
    /// Upper 32 bits of [`Self::Hpmcounter23`], RV32 only.
    pub const HPMCOUNTER23H: CsrSpecifier = 0xC97;
    /// Upper 32 bits of [`Self::Hpmcounter24`], RV32 only.
    pub const HPMCOUNTER24H: CsrSpecifier = 0xC98;
    /// Upper 32 bits of [`Self::Hpmcounter25`], RV32 only.
    pub const HPMCOUNTER25H: CsrSpecifier = 0xC99;
    /// Upper 32 bits of [`Self::Hpmcounter26`], RV32 only.
    pub const HPMCOUNTER26H: CsrSpecifier = 0xC9A;
    /// Upper 32 bits of [`Self::Hpmcounter27`], RV32 only.
    pub const HPMCOUNTER27H: CsrSpecifier = 0xC9B;
    /// Upper 32 bits of [`Self::Hpmcounter28`], RV32 only.
    pub const HPMCOUNTER28H: CsrSpecifier = 0xC9C;
    /// Upper 32 bits of [`Self::Hpmcounter29`], RV32 only.
    pub const HPMCOUNTER29H: CsrSpecifier = 0xC9D;
    /// Upper 32 bits of [`Self::Hpmcounter30`], RV32 only.
    pub const HPMCOUNTER30H: CsrSpecifier = 0xC9E;
    /// Upper 32 bits of [`Self::Hpmcounter31`], RV32 only.
    pub const HPMCOUNTER31H: CsrSpecifier = 0xC9F;

    //
    // Supervisor trap setup (`0x100`, `0x104..=0x106`).
    //
    /// Supervisor status register.
    pub const SSTATUS: CsrSpecifier = 0x100;
    /// Supervisor interrupt-enable register.
    pub const SIE: CsrSpecifier = 0x104;
    /// Supervisor trap handler base address.
    pub const STVEC: CsrSpecifier = 0x105;
    /// Supervisor counter enable.
    pub const SCOUNTEREN: CsrSpecifier = 0x106;

    //
    // Supervisor configuration (`0x10A`).
    //
    /// Supervisor environment configuration register.
    pub const SENVCFG: CsrSpecifier = 0x10A;

    //
    // Supervisor trap handling (`0x140..=0x144`).
    //
    /// Scratch register for supervisor trap handling.
    pub const SSCRATCH: CsrSpecifier = 0x140;
    /// Supervisor exception program counter.
    pub const SEPC: CsrSpecifier = 0x141;
    /// Supervisor trap cause.
    pub const SCAUSE: CsrSpecifier = 0x142;
    /// Supervisor bad address or instruction.
    pub const STVAL: CsrSpecifier = 0x143;
    /// Supervisor interrupt pending.
    pub const SIP: CsrSpecifier = 0x144;

    //
    // Supervisor protection and translation (`0x180`).
    //
    /// Supervisor address translation and protection.
    pub const SATP: CsrSpecifier = 0x180;

    //
    // Debug/trace registers (`0x5A8`).
    //
    /// Supervisor-mode context register.
    pub const SCONTEXT: CsrSpecifier = 0x5A8;

    //
    // Machine information registers (`0xF11..=0xF15`).
    //
    /// Vendor ID.
    pub const MVENDORID: CsrSpecifier = 0xF11;
    /// Architecture ID.
    pub const MARCHID: CsrSpecifier = 0xF12;
    /// Implementation ID.
    pub const MIMPID: CsrSpecifier = 0xF13;
    /// Hardware thead ID.
    pub const MHARTID: CsrSpecifier = 0xF14;
    /// Pointer to configuration data structure.
    pub const MCONFIGPTR: CsrSpecifier = 0xF15;

    //
    // Machine trap setup (`0x300..=0x306`, `0x310`).
    //
    /// Machine status register.
    pub const MSTATUS: CsrSpecifier = 0x300;
    /// ISA and extensions.
    pub const MISA: CsrSpecifier = 0x301;
    /// Machine exception delegation register.
    pub const MEDELEG: CsrSpecifier = 0x302;
    /// Machine interrupt delegation register.
    pub const MIDELEG: CsrSpecifier = 0x303;
    /// Machine interrupt-enable register.
    pub const MIE: CsrSpecifier = 0x304;
    /// Machine trap-handle base address.
    pub const MTVEC: CsrSpecifier = 0x305;
    /// Machine counter enable.
    pub const MCOUNTEREN: CsrSpecifier = 0x306;
    /// Additional machine status register, RV32 only.
    pub const MSTATUSH: CsrSpecifier = 0x310;

    //
    // Machine trap handling (`0x340..=0x344`, `0x34A..=0x34B`).
    //
    /// Scratch register for machine trap handlers.
    pub const MSCRATCH: CsrSpecifier = 0x340;
    /// Machine exception program counter.
    pub const MEPC: CsrSpecifier = 0x341;
    /// Machine trap cause.
    pub const MCAUSE: CsrSpecifier = 0x342;
    /// Machine bad address or instruction.
    pub const MTVAL: CsrSpecifier = 0x343;
    /// Machine interrupt pending.
    pub const MIP: CsrSpecifier = 0x344;
    /// Machine trap instruction (transformed).
    pub const MTINST: CsrSpecifier = 0x34A;
    /// Machine bad guest physical address.
    pub const MTVAL2: CsrSpecifier = 0x34B;

    //
    // Machine configuration (`0x30A`, `0x31A`, `0x747`, `0x757`).
    //
    /// Machine environment configuration register.
    pub const MENVCFG: CsrSpecifier = 0x30A;
    /// Additional machine environment configuration register, RV32 only.
    pub const MENVCFGH: CsrSpecifier = 0x31A;
    /// Machine security configuration register.
    pub const MSECCFG: CsrSpecifier = 0x747;
    /// Additional machine security configuration register, RV32 only.
    pub const MSECCFGH: CsrSpecifier = 0x757;

    //
    // Machine memory protection (`0x3A0..=0x3EF`).
    //
    /// Physical memory protection configuration.
    pub const PMPCFG0: CsrSpecifier = 0x3A0;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG1: CsrSpecifier = 0x3A1;
    /// Physical memory protection configuration.
    pub const PMPCFG2: CsrSpecifier = 0x3A2;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG3: CsrSpecifier = 0x3A3;
    /// Physical memory protection configuration.
    pub const PMPCFG4: CsrSpecifier = 0x3A4;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG5: CsrSpecifier = 0x3A5;
    /// Physical memory protection configuration.
    pub const PMPCFG6: CsrSpecifier = 0x3A6;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG7: CsrSpecifier = 0x3A7;
    /// Physical memory protection configuration.
    pub const PMPCFG8: CsrSpecifier = 0x3A8;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG9: CsrSpecifier = 0x3A9;
    /// Physical memory protection configuration.
    pub const PMPCFG10: CsrSpecifier = 0x3AA;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG11: CsrSpecifier = 0x3AB;
    /// Physical memory protection configuration.
    pub const PMPCFG12: CsrSpecifier = 0x3AC;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG13: CsrSpecifier = 0x3AD;
    /// Physical memory protection configuration.
    pub const PMPCFG14: CsrSpecifier = 0x3AE;
    /// Physical memory protection configuration, RV32 only.
    pub const PMPCFG15: CsrSpecifier = 0x3AF;
    /// Physical memory protection address register.
    pub const PMPADDR0: CsrSpecifier = 0x3B0;
    /// Physical memory protection address register.
    pub const PMPADDR1: CsrSpecifier = 0x3B1;
    /// Physical memory protection address register.
    pub const PMPADDR2: CsrSpecifier = 0x3B2;
    /// Physical memory protection address register.
    pub const PMPADDR3: CsrSpecifier = 0x3B3;
    /// Physical memory protection address register.
    pub const PMPADDR4: CsrSpecifier = 0x3B4;
    /// Physical memory protection address register.
    pub const PMPADDR5: CsrSpecifier = 0x3B5;
    /// Physical memory protection address register.
    pub const PMPADDR6: CsrSpecifier = 0x3B6;
    /// Physical memory protection address register.
    pub const PMPADDR7: CsrSpecifier = 0x3B7;
    /// Physical memory protection address register.
    pub const PMPADDR8: CsrSpecifier = 0x3B8;
    /// Physical memory protection address register.
    pub const PMPADDR9: CsrSpecifier = 0x3B9;
    /// Physical memory protection address register.
    pub const PMPADDR10: CsrSpecifier = 0x3BA;
    /// Physical memory protection address register.
    pub const PMPADDR11: CsrSpecifier = 0x3BB;
    /// Physical memory protection address register.
    pub const PMPADDR12: CsrSpecifier = 0x3BC;
    /// Physical memory protection address register.
    pub const PMPADDR13: CsrSpecifier = 0x3BD;
    /// Physical memory protection address register.
    pub const PMPADDR14: CsrSpecifier = 0x3BE;
    /// Physical memory protection address register.
    pub const PMPADDR15: CsrSpecifier = 0x3BF;
    /// Physical memory protection address register.
    pub const PMPADDR16: CsrSpecifier = 0x3C0;
    /// Physical memory protection address register.
    pub const PMPADDR17: CsrSpecifier = 0x3C1;
    /// Physical memory protection address register.
    pub const PMPADDR18: CsrSpecifier = 0x3C2;
    /// Physical memory protection address register.
    pub const PMPADDR19: CsrSpecifier = 0x3C3;
    /// Physical memory protection address register.
    pub const PMPADDR20: CsrSpecifier = 0x3C4;
    /// Physical memory protection address register.
    pub const PMPADDR21: CsrSpecifier = 0x3C5;
    /// Physical memory protection address register.
    pub const PMPADDR22: CsrSpecifier = 0x3C6;
    /// Physical memory protection address register.
    pub const PMPADDR23: CsrSpecifier = 0x3C7;
    /// Physical memory protection address register.
    pub const PMPADDR24: CsrSpecifier = 0x3C8;
    /// Physical memory protection address register.
    pub const PMPADDR25: CsrSpecifier = 0x3C9;
    /// Physical memory protection address register.
    pub const PMPADDR26: CsrSpecifier = 0x3CA;
    /// Physical memory protection address register.
    pub const PMPADDR27: CsrSpecifier = 0x3CB;
    /// Physical memory protection address register.
    pub const PMPADDR28: CsrSpecifier = 0x3CC;
    /// Physical memory protection address register.
    pub const PMPADDR29: CsrSpecifier = 0x3CD;
    /// Physical memory protection address register.
    pub const PMPADDR30: CsrSpecifier = 0x3CE;
    /// Physical memory protection address register.
    pub const PMPADDR31: CsrSpecifier = 0x3CF;
    /// Physical memory protection address register.
    pub const PMPADDR32: CsrSpecifier = 0x3D0;
    /// Physical memory protection address register.
    pub const PMPADDR33: CsrSpecifier = 0x3D1;
    /// Physical memory protection address register.
    pub const PMPADDR34: CsrSpecifier = 0x3D2;
    /// Physical memory protection address register.
    pub const PMPADDR35: CsrSpecifier = 0x3D3;
    /// Physical memory protection address register.
    pub const PMPADDR36: CsrSpecifier = 0x3D4;
    /// Physical memory protection address register.
    pub const PMPADDR37: CsrSpecifier = 0x3D5;
    /// Physical memory protection address register.
    pub const PMPADDR38: CsrSpecifier = 0x3D6;
    /// Physical memory protection address register.
    pub const PMPADDR39: CsrSpecifier = 0x3D7;
    /// Physical memory protection address register.
    pub const PMPADDR40: CsrSpecifier = 0x3D8;
    /// Physical memory protection address register.
    pub const PMPADDR41: CsrSpecifier = 0x3D9;
    /// Physical memory protection address register.
    pub const PMPADDR42: CsrSpecifier = 0x3DA;
    /// Physical memory protection address register.
    pub const PMPADDR43: CsrSpecifier = 0x3DB;
    /// Physical memory protection address register.
    pub const PMPADDR44: CsrSpecifier = 0x3DC;
    /// Physical memory protection address register.
    pub const PMPADDR45: CsrSpecifier = 0x3DD;
    /// Physical memory protection address register.
    pub const PMPADDR46: CsrSpecifier = 0x3DE;
    /// Physical memory protection address register.
    pub const PMPADDR47: CsrSpecifier = 0x3DF;
    /// Physical memory protection address register.
    pub const PMPADDR48: CsrSpecifier = 0x3E0;
    /// Physical memory protection address register.
    pub const PMPADDR49: CsrSpecifier = 0x3E1;
    /// Physical memory protection address register.
    pub const PMPADDR50: CsrSpecifier = 0x3E2;
    /// Physical memory protection address register.
    pub const PMPADDR51: CsrSpecifier = 0x3E3;
    /// Physical memory protection address register.
    pub const PMPADDR52: CsrSpecifier = 0x3E4;
    /// Physical memory protection address register.
    pub const PMPADDR53: CsrSpecifier = 0x3E5;
    /// Physical memory protection address register.
    pub const PMPADDR54: CsrSpecifier = 0x3E6;
    /// Physical memory protection address register.
    pub const PMPADDR55: CsrSpecifier = 0x3E7;
    /// Physical memory protection address register.
    pub const PMPADDR56: CsrSpecifier = 0x3E8;
    /// Physical memory protection address register.
    pub const PMPADDR57: CsrSpecifier = 0x3E9;
    /// Physical memory protection address register.
    pub const PMPADDR58: CsrSpecifier = 0x3EA;
    /// Physical memory protection address register.
    pub const PMPADDR59: CsrSpecifier = 0x3EB;
    /// Physical memory protection address register.
    pub const PMPADDR60: CsrSpecifier = 0x3EC;
    /// Physical memory protection address register.
    pub const PMPADDR61: CsrSpecifier = 0x3ED;
    /// Physical memory protection address register.
    pub const PMPADDR62: CsrSpecifier = 0x3EE;
    /// Physical memory protection address register.
    pub const PMPADDR63: CsrSpecifier = 0x3EF;

    //
    // Machine counters/timers (`0xB00`, `0xB02..=0xB1F`, `0xB80..=0xB9F`).
    //
    /// Machine cycle counter.
    pub const MCYCLE: CsrSpecifier = 0xB00;
    /// Machine instructions-retired counter.
    pub const MINSTRET: CsrSpecifier = 0xB02;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER3: CsrSpecifier = 0xB03;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER4: CsrSpecifier = 0xB04;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER5: CsrSpecifier = 0xB05;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER6: CsrSpecifier = 0xB06;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER7: CsrSpecifier = 0xB07;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER8: CsrSpecifier = 0xB08;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER9: CsrSpecifier = 0xB09;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER10: CsrSpecifier = 0xB0A;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER11: CsrSpecifier = 0xB0B;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER12: CsrSpecifier = 0xB0C;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER13: CsrSpecifier = 0xB0D;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER14: CsrSpecifier = 0xB0E;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER15: CsrSpecifier = 0xB0F;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER16: CsrSpecifier = 0xB10;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER17: CsrSpecifier = 0xB11;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER18: CsrSpecifier = 0xB12;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER19: CsrSpecifier = 0xB13;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER20: CsrSpecifier = 0xB14;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER21: CsrSpecifier = 0xB15;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER22: CsrSpecifier = 0xB16;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER23: CsrSpecifier = 0xB17;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER24: CsrSpecifier = 0xB18;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER25: CsrSpecifier = 0xB19;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER26: CsrSpecifier = 0xB1A;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER27: CsrSpecifier = 0xB1B;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER28: CsrSpecifier = 0xB1C;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER29: CsrSpecifier = 0xB1D;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER30: CsrSpecifier = 0xB1E;
    /// Machine performance-monitoring counter.
    pub const MHPMCOUNTER31: CsrSpecifier = 0xB1F;
    // RV32-only registers for the upper 32 bits of all machine counter registers
    /// Upper 32 bits of [`Self::Mcycle`], RV32 only.
    pub const MCYCLEH: CsrSpecifier = 0xB80;
    /// Upper 32 bits of [`Self::Minstret`], RV32 only.
    pub const MINSTRETH: CsrSpecifier = 0xB82;
    /// Upper 32 bits of [`Self::Mhpmcounter3`], RV32 only.
    pub const MHPMCOUNTER3H: CsrSpecifier = 0xB83;
    /// Upper 32 bits of [`Self::Mhpmcounter4`], RV32 only.
    pub const MHPMCOUNTER4H: CsrSpecifier = 0xB84;
    /// Upper 32 bits of [`Self::Mhpmcounter5`], RV32 only.
    pub const MHPMCOUNTER5H: CsrSpecifier = 0xB85;
    /// Upper 32 bits of [`Self::Mhpmcounter6`], RV32 only.
    pub const MHPMCOUNTER6H: CsrSpecifier = 0xB86;
    /// Upper 32 bits of [`Self::Mhpmcounter7`], RV32 only.
    pub const MHPMCOUNTER7H: CsrSpecifier = 0xB87;
    /// Upper 32 bits of [`Self::Mhpmcounter8`], RV32 only.
    pub const MHPMCOUNTER8H: CsrSpecifier = 0xB88;
    /// Upper 32 bits of [`Self::Mhpmcounter9`], RV32 only.
    pub const MHPMCOUNTER9H: CsrSpecifier = 0xB89;
    /// Upper 32 bits of [`Self::Mhpmcounter10`], RV32 only.
    pub const MHPMCOUNTER10H: CsrSpecifier = 0xB8A;
    /// Upper 32 bits of [`Self::Mhpmcounter11`], RV32 only.
    pub const MHPMCOUNTER11H: CsrSpecifier = 0xB8B;
    /// Upper 32 bits of [`Self::Mhpmcounter12`], RV32 only.
    pub const MHPMCOUNTER12H: CsrSpecifier = 0xB8C;
    /// Upper 32 bits of [`Self::Mhpmcounter13`], RV32 only.
    pub const MHPMCOUNTER13H: CsrSpecifier = 0xB8D;
    /// Upper 32 bits of [`Self::Mhpmcounter14`], RV32 only.
    pub const MHPMCOUNTER14H: CsrSpecifier = 0xB8E;
    /// Upper 32 bits of [`Self::Mhpmcounter15`], RV32 only.
    pub const MHPMCOUNTER15H: CsrSpecifier = 0xB8F;
    /// Upper 32 bits of [`Self::Mhpmcounter16`], RV32 only.
    pub const MHPMCOUNTER16H: CsrSpecifier = 0xB90;
    /// Upper 32 bits of [`Self::Mhpmcounter17`], RV32 only.
    pub const MHPMCOUNTER17H: CsrSpecifier = 0xB91;
    /// Upper 32 bits of [`Self::Mhpmcounter18`], RV32 only.
    pub const MHPMCOUNTER18H: CsrSpecifier = 0xB92;
    /// Upper 32 bits of [`Self::Mhpmcounter19`], RV32 only.
    pub const MHPMCOUNTER19H: CsrSpecifier = 0xB93;
    /// Upper 32 bits of [`Self::Mhpmcounter20`], RV32 only.
    pub const MHPMCOUNTER20H: CsrSpecifier = 0xB94;
    /// Upper 32 bits of [`Self::Mhpmcounter21`], RV32 only.
    pub const MHPMCOUNTER21H: CsrSpecifier = 0xB95;
    /// Upper 32 bits of [`Self::Mhpmcounter22`], RV32 only.
    pub const MHPMCOUNTER22H: CsrSpecifier = 0xB96;
    /// Upper 32 bits of [`Self::Mhpmcounter23`], RV32 only.
    pub const MHPMCOUNTER23H: CsrSpecifier = 0xB97;
    /// Upper 32 bits of [`Self::Mhpmcounter24`], RV32 only.
    pub const MHPMCOUNTER24H: CsrSpecifier = 0xB98;
    /// Upper 32 bits of [`Self::Mhpmcounter25`], RV32 only.
    pub const MHPMCOUNTER25H: CsrSpecifier = 0xB99;
    /// Upper 32 bits of [`Self::Mhpmcounter26`], RV32 only.
    pub const MHPMCOUNTER26H: CsrSpecifier = 0xB9A;
    /// Upper 32 bits of [`Self::Mhpmcounter27`], RV32 only.
    pub const MHPMCOUNTER27H: CsrSpecifier = 0xB9B;
    /// Upper 32 bits of [`Self::Mhpmcounter28`], RV32 only.
    pub const MHPMCOUNTER28H: CsrSpecifier = 0xB9C;
    /// Upper 32 bits of [`Self::Mhpmcounter29`], RV32 only.
    pub const MHPMCOUNTER29H: CsrSpecifier = 0xB9D;
    /// Upper 32 bits of [`Self::Mhpmcounter30`], RV32 only.
    pub const MHPMCOUNTER30H: CsrSpecifier = 0xB9E;
    /// Upper 32 bits of [`Self::Mhpmcounter31`], RV32 only.
    pub const MHPMCOUNTER31H: CsrSpecifier = 0xB9F;

    //
    // Machine counter setup (`0x320`, `0x323..=0x33F`)
    //
    /// Machine counter-inhibit register.
    pub const MCOUNTINHIBIT: CsrSpecifier = 0x320;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT3: CsrSpecifier = 0x323;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT4: CsrSpecifier = 0x324;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT5: CsrSpecifier = 0x325;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT6: CsrSpecifier = 0x326;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT7: CsrSpecifier = 0x327;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT8: CsrSpecifier = 0x328;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT9: CsrSpecifier = 0x329;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT10: CsrSpecifier = 0x32A;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT11: CsrSpecifier = 0x32B;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT12: CsrSpecifier = 0x32C;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT13: CsrSpecifier = 0x32D;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT14: CsrSpecifier = 0x32E;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT15: CsrSpecifier = 0x32F;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT16: CsrSpecifier = 0x330;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT17: CsrSpecifier = 0x331;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT18: CsrSpecifier = 0x332;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT19: CsrSpecifier = 0x333;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT20: CsrSpecifier = 0x334;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT21: CsrSpecifier = 0x335;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT22: CsrSpecifier = 0x336;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT23: CsrSpecifier = 0x337;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT24: CsrSpecifier = 0x338;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT25: CsrSpecifier = 0x339;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT26: CsrSpecifier = 0x33A;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT27: CsrSpecifier = 0x33B;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT28: CsrSpecifier = 0x33C;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT29: CsrSpecifier = 0x33D;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT30: CsrSpecifier = 0x33E;
    /// Machine performance-monitoring event selector.
    pub const MHPMEVENT31: CsrSpecifier = 0x33F;

    //
    // Debug/trace registers (`0x7A0..=0x7A3`, `0x7A8`)
    //
    /// Debug/trace trigger register select.
    pub const TSELECT: CsrSpecifier = 0x7A0;
    /// First debug/trace trigger data register.
    pub const TDATA1: CsrSpecifier = 0x7A1;
    /// Second debug/trace trigger data register.
    pub const TDATA2: CsrSpecifier = 0x7A2;
    /// Third debug/trace trigger data register.
    pub const TDATA3: CsrSpecifier = 0x7A3;
    /// Machine-mode context register.
    pub const MCONTEXT: CsrSpecifier = 0x7A8;

    /// Returns `true` if `specifier` is valid, which is the case if it fits in 12 bits.
    pub fn is_valid(specifier: CsrSpecifier) -> bool {
        specifier < 1 << 12
    }

    /// Returns `true` if this CSR only supports read access.
    ///
    /// Requires [`is_valid(specifier)`](is_valid), otherwise the return value is undefined.
    pub fn is_read_only(specifier: CsrSpecifier) -> bool {
        // The top two bits of a CSR specifier indicate whether the CSR is read-only (0b11) or
        // read/write (0b00, 0b01, 0b10)
        specifier >> 10 == 0b11
    }

    /// Returns the minimum required privilege level to access this CSR.
    ///
    /// Requires [`is_valid(specifier)`](is_valid), otherwise the return value is undefined.
    ///
    /// Note that this returns a [`RawPrivilegeLevel`], meaning the minimum required privilege level
    /// may be a reserved level. This still has a defined meaning: only higher privilege levels are
    /// allowed to access the CSR.
    pub fn required_privilege_level(specifier: CsrSpecifier) -> RawPrivilegeLevel {
        // Bits `10:9` indicate the minimum required privilege level
        RawPrivilegeLevel::from_u2(((specifier >> 8) & 0b11) as u8)
    }
}
