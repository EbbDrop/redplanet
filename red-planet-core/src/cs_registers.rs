//! Allocated Control and Status Registers.
//!
//! Part of the "Zicsr" extension.

use crate::{Allocator, PrivilegeLevel, RawPrivilegeLevel};

/// Control and Status Registers for a single RV32I hart
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
    counters: A::Id<[u64; 32]>,
}

impl<A: Allocator> CSRegisters<A> {
    /// Creates a fresh collection of registers initialized to their reset values.
    pub fn new(allocator: &mut A) -> Self {
        Self {
            counters: allocator.insert([0; 32]),
        }
    }

    /// Force all Control and Status registers to their reset state.
    pub fn reset(&self, allocator: &mut A) {
        *allocator.get_mut(self.counters).unwrap() = [0; 32];
    }

    /// Read the value of a CSR by its specifier.
    ///
    /// `privilege_level` indicates at what privilege level the read is performed. If the CSR that
    /// is being read requires a higher privilege level (see
    /// [`Specifier::required_privilege_level`]), then an [`AccessError::Privileged`] will be given.
    pub fn read(
        &self,
        allocator: &A,
        specifier: Specifier,
        privilege_level: PrivilegeLevel,
    ) -> Result<u32, AccessError> {
        let required_level = specifier.required_privilege_level();
        if privilege_level < required_level {
            return Err(AccessError::Privileged { required_level });
        }
        match specifier {
            Specifier::Cycle
            | Specifier::Time
            | Specifier::Instret
            | Specifier::Hpmcounter3
            | Specifier::Hpmcounter4
            | Specifier::Hpmcounter5
            | Specifier::Hpmcounter6
            | Specifier::Hpmcounter7
            | Specifier::Hpmcounter8
            | Specifier::Hpmcounter9
            | Specifier::Hpmcounter10
            | Specifier::Hpmcounter11
            | Specifier::Hpmcounter12
            | Specifier::Hpmcounter13
            | Specifier::Hpmcounter14
            | Specifier::Hpmcounter15
            | Specifier::Hpmcounter16
            | Specifier::Hpmcounter17
            | Specifier::Hpmcounter18
            | Specifier::Hpmcounter19
            | Specifier::Hpmcounter20
            | Specifier::Hpmcounter21
            | Specifier::Hpmcounter22
            | Specifier::Hpmcounter23
            | Specifier::Hpmcounter24
            | Specifier::Hpmcounter25
            | Specifier::Hpmcounter26
            | Specifier::Hpmcounter27
            | Specifier::Hpmcounter28
            | Specifier::Hpmcounter29
            | Specifier::Hpmcounter30
            | Specifier::Hpmcounter31 => {
                let offset = specifier as usize - Specifier::Cycle as usize;
                Ok(allocator.get(self.counters).unwrap()[offset] as u32)
            }
            Specifier::Cycleh
            | Specifier::Timeh
            | Specifier::Instreth
            | Specifier::Hpmcounter3h
            | Specifier::Hpmcounter4h
            | Specifier::Hpmcounter5h
            | Specifier::Hpmcounter6h
            | Specifier::Hpmcounter7h
            | Specifier::Hpmcounter8h
            | Specifier::Hpmcounter9h
            | Specifier::Hpmcounter10h
            | Specifier::Hpmcounter11h
            | Specifier::Hpmcounter12h
            | Specifier::Hpmcounter13h
            | Specifier::Hpmcounter14h
            | Specifier::Hpmcounter15h
            | Specifier::Hpmcounter16h
            | Specifier::Hpmcounter17h
            | Specifier::Hpmcounter18h
            | Specifier::Hpmcounter19h
            | Specifier::Hpmcounter20h
            | Specifier::Hpmcounter21h
            | Specifier::Hpmcounter22h
            | Specifier::Hpmcounter23h
            | Specifier::Hpmcounter24h
            | Specifier::Hpmcounter25h
            | Specifier::Hpmcounter26h
            | Specifier::Hpmcounter27h
            | Specifier::Hpmcounter28h
            | Specifier::Hpmcounter29h
            | Specifier::Hpmcounter30h
            | Specifier::Hpmcounter31h => {
                let offset = specifier as usize - Specifier::Cycleh as usize;
                Ok((allocator.get(self.counters).unwrap()[offset] >> 32) as u32)
            }
            Specifier::Misa => todo!(),
            _ => todo!(),
        }
    }

    pub fn write(
        &self,
        _allocator: &mut A,
        specifier: Specifier,
        privilege_level: PrivilegeLevel,
    ) -> Result<u32, WriteError> {
        let required_level = specifier.required_privilege_level();
        if privilege_level < required_level {
            return Err(WriteError::AccessError(AccessError::Privileged {
                required_level,
            }));
        }
        if specifier.is_read_only() {
            return Err(WriteError::WriteToReadOnly);
        }
        todo!()
    }
}

/// Errors that can occur when attempting to access a CSR.
#[derive(Debug)]
pub enum AccessError {
    /// Attempt to access a CSR that requires a higher privilege level.
    Privileged {
        /// The minimum required privilege level to access that CSR.
        required_level: RawPrivilegeLevel,
    },
}

/// Errors that can occur when attempting to write to a CSR.
#[derive(Debug)]
pub enum WriteError {
    /// A non-write specific access error. See [`AccessError`].
    AccessError(AccessError),
    /// Attempt to write to a read-only register.
    WriteToReadOnly,
}

/// Specifiers for all supported CSRs
///
/// Debug-mode CSRs are not supported.
/// The hypervisor extension is also not supported.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u16)]
pub enum Specifier {
    //
    // Unprivileged floating-point CSRs (`0x001..=0x003`).
    //
    /// Floating-point accrued exceptions.
    Fflags = 0x001,
    /// Floating-point dynamic rounding mode.
    Frm = 0x002,
    /// Floating-point CSR ([`Self::Frm`] + [`Self::Fflags`]).
    Fcsr = 0x003,

    //
    // Unprivileged counters/timers (`0xC00..=0xC1F`, `0xC80..=0xC9F`).
    //
    /// Cycle counter for RDCYCLE instruction.
    Cycle = 0xC00,
    /// Timer for RDTIME instruction.
    Time = 0xC01,
    /// Instructions-retired counter for RDINSTRET instruction.
    Instret = 0xC02,
    /// Performance-monitoring counter.
    Hpmcounter3 = 0xC03,
    /// Performance-monitoring counter.
    Hpmcounter4 = 0xC04,
    /// Performance-monitoring counter.
    Hpmcounter5 = 0xC05,
    /// Performance-monitoring counter.
    Hpmcounter6 = 0xC06,
    /// Performance-monitoring counter.
    Hpmcounter7 = 0xC07,
    /// Performance-monitoring counter.
    Hpmcounter8 = 0xC08,
    /// Performance-monitoring counter.
    Hpmcounter9 = 0xC09,
    /// Performance-monitoring counter.
    Hpmcounter10 = 0xC0A,
    /// Performance-monitoring counter.
    Hpmcounter11 = 0xC0B,
    /// Performance-monitoring counter.
    Hpmcounter12 = 0xC0C,
    /// Performance-monitoring counter.
    Hpmcounter13 = 0xC0D,
    /// Performance-monitoring counter.
    Hpmcounter14 = 0xC0E,
    /// Performance-monitoring counter.
    Hpmcounter15 = 0xC0F,
    /// Performance-monitoring counter.
    Hpmcounter16 = 0xC10,
    /// Performance-monitoring counter.
    Hpmcounter17 = 0xC11,
    /// Performance-monitoring counter.
    Hpmcounter18 = 0xC12,
    /// Performance-monitoring counter.
    Hpmcounter19 = 0xC13,
    /// Performance-monitoring counter.
    Hpmcounter20 = 0xC14,
    /// Performance-monitoring counter.
    Hpmcounter21 = 0xC15,
    /// Performance-monitoring counter.
    Hpmcounter22 = 0xC16,
    /// Performance-monitoring counter.
    Hpmcounter23 = 0xC17,
    /// Performance-monitoring counter.
    Hpmcounter24 = 0xC18,
    /// Performance-monitoring counter.
    Hpmcounter25 = 0xC19,
    /// Performance-monitoring counter.
    Hpmcounter26 = 0xC1A,
    /// Performance-monitoring counter.
    Hpmcounter27 = 0xC1B,
    /// Performance-monitoring counter.
    Hpmcounter28 = 0xC1C,
    /// Performance-monitoring counter.
    Hpmcounter29 = 0xC1D,
    /// Performance-monitoring counter.
    Hpmcounter30 = 0xC1E,
    /// Performance-monitoring counter.
    Hpmcounter31 = 0xC1F,
    // RV32-only registers for the upper 32 bits of all counter registers
    /// Upper 32 bits of [`Self::Cycle`], RV32 only.
    Cycleh = 0xC80,
    /// Upper 32 bits of [`Self::Time`], RV32 only.
    Timeh = 0xC81,
    /// Upper 32 bits of [`Self::Instret`], RV32 only.
    Instreth = 0xC82,
    /// Upper 32 bits of [`Self::Hpmcounter3`], RV32 only.
    Hpmcounter3h = 0xC83,
    /// Upper 32 bits of [`Self::Hpmcounter4`], RV32 only.
    Hpmcounter4h = 0xC84,
    /// Upper 32 bits of [`Self::Hpmcounter5`], RV32 only.
    Hpmcounter5h = 0xC85,
    /// Upper 32 bits of [`Self::Hpmcounter6`], RV32 only.
    Hpmcounter6h = 0xC86,
    /// Upper 32 bits of [`Self::Hpmcounter7`], RV32 only.
    Hpmcounter7h = 0xC87,
    /// Upper 32 bits of [`Self::Hpmcounter8`], RV32 only.
    Hpmcounter8h = 0xC88,
    /// Upper 32 bits of [`Self::Hpmcounter9`], RV32 only.
    Hpmcounter9h = 0xC89,
    /// Upper 32 bits of [`Self::Hpmcounter10`], RV32 only.
    Hpmcounter10h = 0xC8A,
    /// Upper 32 bits of [`Self::Hpmcounter11`], RV32 only.
    Hpmcounter11h = 0xC8B,
    /// Upper 32 bits of [`Self::Hpmcounter12`], RV32 only.
    Hpmcounter12h = 0xC8C,
    /// Upper 32 bits of [`Self::Hpmcounter13`], RV32 only.
    Hpmcounter13h = 0xC8D,
    /// Upper 32 bits of [`Self::Hpmcounter14`], RV32 only.
    Hpmcounter14h = 0xC8E,
    /// Upper 32 bits of [`Self::Hpmcounter15`], RV32 only.
    Hpmcounter15h = 0xC8F,
    /// Upper 32 bits of [`Self::Hpmcounter16`], RV32 only.
    Hpmcounter16h = 0xC90,
    /// Upper 32 bits of [`Self::Hpmcounter17`], RV32 only.
    Hpmcounter17h = 0xC91,
    /// Upper 32 bits of [`Self::Hpmcounter18`], RV32 only.
    Hpmcounter18h = 0xC92,
    /// Upper 32 bits of [`Self::Hpmcounter19`], RV32 only.
    Hpmcounter19h = 0xC93,
    /// Upper 32 bits of [`Self::Hpmcounter20`], RV32 only.
    Hpmcounter20h = 0xC94,
    /// Upper 32 bits of [`Self::Hpmcounter21`], RV32 only.
    Hpmcounter21h = 0xC95,
    /// Upper 32 bits of [`Self::Hpmcounter22`], RV32 only.
    Hpmcounter22h = 0xC96,
    /// Upper 32 bits of [`Self::Hpmcounter23`], RV32 only.
    Hpmcounter23h = 0xC97,
    /// Upper 32 bits of [`Self::Hpmcounter24`], RV32 only.
    Hpmcounter24h = 0xC98,
    /// Upper 32 bits of [`Self::Hpmcounter25`], RV32 only.
    Hpmcounter25h = 0xC99,
    /// Upper 32 bits of [`Self::Hpmcounter26`], RV32 only.
    Hpmcounter26h = 0xC9A,
    /// Upper 32 bits of [`Self::Hpmcounter27`], RV32 only.
    Hpmcounter27h = 0xC9B,
    /// Upper 32 bits of [`Self::Hpmcounter28`], RV32 only.
    Hpmcounter28h = 0xC9C,
    /// Upper 32 bits of [`Self::Hpmcounter29`], RV32 only.
    Hpmcounter29h = 0xC9D,
    /// Upper 32 bits of [`Self::Hpmcounter30`], RV32 only.
    Hpmcounter30h = 0xC9E,
    /// Upper 32 bits of [`Self::Hpmcounter31`], RV32 only.
    Hpmcounter31h = 0xC9F,

    //
    // Supervisor trap setup (`0x100`, `0x104..=0x106`).
    //
    /// Supervisor status register.
    Sstatus = 0x100,
    /// Supervisor interrupt-enable register.
    Sie = 0x104,
    /// Supervisor trap handler base address.
    Stvec = 0x105,
    /// Supervisor counter enable.
    Scounteren = 0x106,

    //
    // Supervisor configuration (`0x10A`).
    //
    /// Supervisor environment configuration register.
    Senvcfg = 0x10A,

    //
    // Supervisor trap handling (`0x140..=0x144`).
    //
    /// Scratch register for supervisor trap handling.
    Sscratch = 0x140,
    /// Supervisor exception program counter.
    Sepc = 0x141,
    /// Supervisor trap cause.
    Scause = 0x142,
    /// Supervisor bad address or instruction.
    Stval = 0x143,
    /// Supervisor interrupt pending.
    Sip = 0x144,

    //
    // Supervisor protection and translation (`0x180`).
    //
    /// Supervisor address translation and protection.
    Satp = 0x180,

    //
    // Debug/trace registers (`0x5A8`).
    //
    /// Supervisor-mode context register.
    Scontext = 0x5A8,

    //
    // Machine information registers (`0xF11..=0xF15`).
    //
    /// Vendor ID.
    Mvendorid = 0xF11,
    /// Architecture ID.
    Marchid = 0xF12,
    /// Implementation ID.
    Mimpid = 0xF13,
    /// Hardware thead ID.
    Mhartid = 0xF14,
    /// Pointer to configuration data structure.
    Mconfigptr = 0xF15,

    //
    // Machine trap setup (`0x300..=0x306`, `0x310`).
    //
    /// Machine status register.
    Mstatus = 0x300,
    /// ISA and extensions.
    Misa = 0x301,
    /// Machine exception delegation register.
    Medeleg = 0x302,
    /// Machine interrupt delegation register.
    Mideleg = 0x303,
    /// Machine interrupt-enable register.
    Mie = 0x304,
    /// Machine trap-handle base address.
    Mtvec = 0x305,
    /// Machine counter enable.
    Mcounteren = 0x306,
    /// Additional machine status register, RV32 only.
    Mstatush = 0x310,

    //
    // Machine trap handling (`0x340..=0x344`, `0x34A..=0x34B`).
    //
    /// Scratch register for machine trap handlers.
    Mscratch = 0x340,
    /// Machine exception program counter.
    Mepc = 0x341,
    /// Machine trap cause.
    Mcause = 0x342,
    /// Machine bad address or instruction.
    Mtval = 0x343,
    /// Machine interrupt pending.
    Mip = 0x344,
    /// Machine trap instruction (transformed).
    Mtinst = 0x34A,
    /// Machine bad guest physical address.
    Mtval2 = 0x34B,

    //
    // Machine configuration (`0x30A`, `0x31A`, `0x747`, `0x757`).
    //
    /// Machine environment configuration register.
    Menvcfg = 0x30A,
    /// Additional machine environment configuration register, RV32 only.
    Menvcfgh = 0x31A,
    /// Machine security configuration register.
    Mseccfg = 0x747,
    /// Additional machine security configuration register, RV32 only.
    Mseccfgh = 0x757,

    //
    // Machine memory protection (`0x3A0..=0x3EF`).
    //
    /// Physical memory protection configuration.
    Pmpcfg0 = 0x3A0,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg1 = 0x3A1,
    /// Physical memory protection configuration.
    Pmpcfg2 = 0x3A2,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg3 = 0x3A3,
    /// Physical memory protection configuration.
    Pmpcfg4 = 0x3A4,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg5 = 0x3A5,
    /// Physical memory protection configuration.
    Pmpcfg6 = 0x3A6,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg7 = 0x3A7,
    /// Physical memory protection configuration.
    Pmpcfg8 = 0x3A8,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg9 = 0x3A9,
    /// Physical memory protection configuration.
    Pmpcfg10 = 0x3AA,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg11 = 0x3AB,
    /// Physical memory protection configuration.
    Pmpcfg12 = 0x3AC,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg13 = 0x3AD,
    /// Physical memory protection configuration.
    Pmpcfg14 = 0x3AE,
    /// Physical memory protection configuration, RV32 only.
    Pmpcfg15 = 0x3AF,
    /// Physical memory protection address register.
    Pmpaddr0 = 0x3B0,
    /// Physical memory protection address register.
    Pmpaddr1 = 0x3B1,
    /// Physical memory protection address register.
    Pmpaddr2 = 0x3B2,
    /// Physical memory protection address register.
    Pmpaddr3 = 0x3B3,
    /// Physical memory protection address register.
    Pmpaddr4 = 0x3B4,
    /// Physical memory protection address register.
    Pmpaddr5 = 0x3B5,
    /// Physical memory protection address register.
    Pmpaddr6 = 0x3B6,
    /// Physical memory protection address register.
    Pmpaddr7 = 0x3B7,
    /// Physical memory protection address register.
    Pmpaddr8 = 0x3B8,
    /// Physical memory protection address register.
    Pmpaddr9 = 0x3B9,
    /// Physical memory protection address register.
    Pmpaddr10 = 0x3BA,
    /// Physical memory protection address register.
    Pmpaddr11 = 0x3BB,
    /// Physical memory protection address register.
    Pmpaddr12 = 0x3BC,
    /// Physical memory protection address register.
    Pmpaddr13 = 0x3BD,
    /// Physical memory protection address register.
    Pmpaddr14 = 0x3BE,
    /// Physical memory protection address register.
    Pmpaddr15 = 0x3BF,
    /// Physical memory protection address register.
    Pmpaddr16 = 0x3C0,
    /// Physical memory protection address register.
    Pmpaddr17 = 0x3C1,
    /// Physical memory protection address register.
    Pmpaddr18 = 0x3C2,
    /// Physical memory protection address register.
    Pmpaddr19 = 0x3C3,
    /// Physical memory protection address register.
    Pmpaddr20 = 0x3C4,
    /// Physical memory protection address register.
    Pmpaddr21 = 0x3C5,
    /// Physical memory protection address register.
    Pmpaddr22 = 0x3C6,
    /// Physical memory protection address register.
    Pmpaddr23 = 0x3C7,
    /// Physical memory protection address register.
    Pmpaddr24 = 0x3C8,
    /// Physical memory protection address register.
    Pmpaddr25 = 0x3C9,
    /// Physical memory protection address register.
    Pmpaddr26 = 0x3CA,
    /// Physical memory protection address register.
    Pmpaddr27 = 0x3CB,
    /// Physical memory protection address register.
    Pmpaddr28 = 0x3CC,
    /// Physical memory protection address register.
    Pmpaddr29 = 0x3CD,
    /// Physical memory protection address register.
    Pmpaddr30 = 0x3CE,
    /// Physical memory protection address register.
    Pmpaddr31 = 0x3CF,
    /// Physical memory protection address register.
    Pmpaddr32 = 0x3D0,
    /// Physical memory protection address register.
    Pmpaddr33 = 0x3D1,
    /// Physical memory protection address register.
    Pmpaddr34 = 0x3D2,
    /// Physical memory protection address register.
    Pmpaddr35 = 0x3D3,
    /// Physical memory protection address register.
    Pmpaddr36 = 0x3D4,
    /// Physical memory protection address register.
    Pmpaddr37 = 0x3D5,
    /// Physical memory protection address register.
    Pmpaddr38 = 0x3D6,
    /// Physical memory protection address register.
    Pmpaddr39 = 0x3D7,
    /// Physical memory protection address register.
    Pmpaddr40 = 0x3D8,
    /// Physical memory protection address register.
    Pmpaddr41 = 0x3D9,
    /// Physical memory protection address register.
    Pmpaddr42 = 0x3DA,
    /// Physical memory protection address register.
    Pmpaddr43 = 0x3DB,
    /// Physical memory protection address register.
    Pmpaddr44 = 0x3DC,
    /// Physical memory protection address register.
    Pmpaddr45 = 0x3DD,
    /// Physical memory protection address register.
    Pmpaddr46 = 0x3DE,
    /// Physical memory protection address register.
    Pmpaddr47 = 0x3DF,
    /// Physical memory protection address register.
    Pmpaddr48 = 0x3E0,
    /// Physical memory protection address register.
    Pmpaddr49 = 0x3E1,
    /// Physical memory protection address register.
    Pmpaddr50 = 0x3E2,
    /// Physical memory protection address register.
    Pmpaddr51 = 0x3E3,
    /// Physical memory protection address register.
    Pmpaddr52 = 0x3E4,
    /// Physical memory protection address register.
    Pmpaddr53 = 0x3E5,
    /// Physical memory protection address register.
    Pmpaddr54 = 0x3E6,
    /// Physical memory protection address register.
    Pmpaddr55 = 0x3E7,
    /// Physical memory protection address register.
    Pmpaddr56 = 0x3E8,
    /// Physical memory protection address register.
    Pmpaddr57 = 0x3E9,
    /// Physical memory protection address register.
    Pmpaddr58 = 0x3EA,
    /// Physical memory protection address register.
    Pmpaddr59 = 0x3EB,
    /// Physical memory protection address register.
    Pmpaddr60 = 0x3EC,
    /// Physical memory protection address register.
    Pmpaddr61 = 0x3ED,
    /// Physical memory protection address register.
    Pmpaddr62 = 0x3EE,
    /// Physical memory protection address register.
    Pmpaddr63 = 0x3EF,

    //
    // Machine counters/timers (`0xB00`, `0xB02..=0xB1F`, `0xB80..=0xB9F`).
    //
    /// Machine cycle counter.
    Mcycle = 0xB00,
    /// Machine instructions-retired counter.
    Minstret = 0xB02,
    /// Machine performance-monitoring counter.
    Mhpmcounter3 = 0xB03,
    /// Machine performance-monitoring counter.
    Mhpmcounter4 = 0xB04,
    /// Machine performance-monitoring counter.
    Mhpmcounter5 = 0xB05,
    /// Machine performance-monitoring counter.
    Mhpmcounter6 = 0xB06,
    /// Machine performance-monitoring counter.
    Mhpmcounter7 = 0xB07,
    /// Machine performance-monitoring counter.
    Mhpmcounter8 = 0xB08,
    /// Machine performance-monitoring counter.
    Mhpmcounter9 = 0xB09,
    /// Machine performance-monitoring counter.
    Mhpmcounter10 = 0xB0A,
    /// Machine performance-monitoring counter.
    Mhpmcounter11 = 0xB0B,
    /// Machine performance-monitoring counter.
    Mhpmcounter12 = 0xB0C,
    /// Machine performance-monitoring counter.
    Mhpmcounter13 = 0xB0D,
    /// Machine performance-monitoring counter.
    Mhpmcounter14 = 0xB0E,
    /// Machine performance-monitoring counter.
    Mhpmcounter15 = 0xB0F,
    /// Machine performance-monitoring counter.
    Mhpmcounter16 = 0xB10,
    /// Machine performance-monitoring counter.
    Mhpmcounter17 = 0xB11,
    /// Machine performance-monitoring counter.
    Mhpmcounter18 = 0xB12,
    /// Machine performance-monitoring counter.
    Mhpmcounter19 = 0xB13,
    /// Machine performance-monitoring counter.
    Mhpmcounter20 = 0xB14,
    /// Machine performance-monitoring counter.
    Mhpmcounter21 = 0xB15,
    /// Machine performance-monitoring counter.
    Mhpmcounter22 = 0xB16,
    /// Machine performance-monitoring counter.
    Mhpmcounter23 = 0xB17,
    /// Machine performance-monitoring counter.
    Mhpmcounter24 = 0xB18,
    /// Machine performance-monitoring counter.
    Mhpmcounter25 = 0xB19,
    /// Machine performance-monitoring counter.
    Mhpmcounter26 = 0xB1A,
    /// Machine performance-monitoring counter.
    Mhpmcounter27 = 0xB1B,
    /// Machine performance-monitoring counter.
    Mhpmcounter28 = 0xB1C,
    /// Machine performance-monitoring counter.
    Mhpmcounter29 = 0xB1D,
    /// Machine performance-monitoring counter.
    Mhpmcounter30 = 0xB1E,
    /// Machine performance-monitoring counter.
    Mhpmcounter31 = 0xB1F,
    // RV32-only registers for the upper 32 bits of all machine counter registers
    /// Upper 32 bits of [`Self::Mcycle`], RV32 only.
    Mcycleh = 0xB80,
    /// Upper 32 bits of [`Self::Minstret`], RV32 only.
    Minstreth = 0xB82,
    /// Upper 32 bits of [`Self::Mhpmcounter3`], RV32 only.
    Mhpmcounter3h = 0xB83,
    /// Upper 32 bits of [`Self::Mhpmcounter4`], RV32 only.
    Mhpmcounter4h = 0xB84,
    /// Upper 32 bits of [`Self::Mhpmcounter5`], RV32 only.
    Mhpmcounter5h = 0xB85,
    /// Upper 32 bits of [`Self::Mhpmcounter6`], RV32 only.
    Mhpmcounter6h = 0xB86,
    /// Upper 32 bits of [`Self::Mhpmcounter7`], RV32 only.
    Mhpmcounter7h = 0xB87,
    /// Upper 32 bits of [`Self::Mhpmcounter8`], RV32 only.
    Mhpmcounter8h = 0xB88,
    /// Upper 32 bits of [`Self::Mhpmcounter9`], RV32 only.
    Mhpmcounter9h = 0xB89,
    /// Upper 32 bits of [`Self::Mhpmcounter10`], RV32 only.
    Mhpmcounter10h = 0xB8A,
    /// Upper 32 bits of [`Self::Mhpmcounter11`], RV32 only.
    Mhpmcounter11h = 0xB8B,
    /// Upper 32 bits of [`Self::Mhpmcounter12`], RV32 only.
    Mhpmcounter12h = 0xB8C,
    /// Upper 32 bits of [`Self::Mhpmcounter13`], RV32 only.
    Mhpmcounter13h = 0xB8D,
    /// Upper 32 bits of [`Self::Mhpmcounter14`], RV32 only.
    Mhpmcounter14h = 0xB8E,
    /// Upper 32 bits of [`Self::Mhpmcounter15`], RV32 only.
    Mhpmcounter15h = 0xB8F,
    /// Upper 32 bits of [`Self::Mhpmcounter16`], RV32 only.
    Mhpmcounter16h = 0xB90,
    /// Upper 32 bits of [`Self::Mhpmcounter17`], RV32 only.
    Mhpmcounter17h = 0xB91,
    /// Upper 32 bits of [`Self::Mhpmcounter18`], RV32 only.
    Mhpmcounter18h = 0xB92,
    /// Upper 32 bits of [`Self::Mhpmcounter19`], RV32 only.
    Mhpmcounter19h = 0xB93,
    /// Upper 32 bits of [`Self::Mhpmcounter20`], RV32 only.
    Mhpmcounter20h = 0xB94,
    /// Upper 32 bits of [`Self::Mhpmcounter21`], RV32 only.
    Mhpmcounter21h = 0xB95,
    /// Upper 32 bits of [`Self::Mhpmcounter22`], RV32 only.
    Mhpmcounter22h = 0xB96,
    /// Upper 32 bits of [`Self::Mhpmcounter23`], RV32 only.
    Mhpmcounter23h = 0xB97,
    /// Upper 32 bits of [`Self::Mhpmcounter24`], RV32 only.
    Mhpmcounter24h = 0xB98,
    /// Upper 32 bits of [`Self::Mhpmcounter25`], RV32 only.
    Mhpmcounter25h = 0xB99,
    /// Upper 32 bits of [`Self::Mhpmcounter26`], RV32 only.
    Mhpmcounter26h = 0xB9A,
    /// Upper 32 bits of [`Self::Mhpmcounter27`], RV32 only.
    Mhpmcounter27h = 0xB9B,
    /// Upper 32 bits of [`Self::Mhpmcounter28`], RV32 only.
    Mhpmcounter28h = 0xB9C,
    /// Upper 32 bits of [`Self::Mhpmcounter29`], RV32 only.
    Mhpmcounter29h = 0xB9D,
    /// Upper 32 bits of [`Self::Mhpmcounter30`], RV32 only.
    Mhpmcounter30h = 0xB9E,
    /// Upper 32 bits of [`Self::Mhpmcounter31`], RV32 only.
    Mhpmcounter31h = 0xB9F,

    //
    // Machine counter setup (`0x320`, `0x323..=0x33F`)
    //
    /// Machine counter-inhibit register.
    Mcountinhibit = 0x320,
    /// Machine performance-monitoring event selector.
    Mhpmevent3 = 0x323,
    /// Machine performance-monitoring event selector.
    Mhpmevent4 = 0x324,
    /// Machine performance-monitoring event selector.
    Mhpmevent5 = 0x325,
    /// Machine performance-monitoring event selector.
    Mhpmevent6 = 0x326,
    /// Machine performance-monitoring event selector.
    Mhpmevent7 = 0x327,
    /// Machine performance-monitoring event selector.
    Mhpmevent8 = 0x328,
    /// Machine performance-monitoring event selector.
    Mhpmevent9 = 0x329,
    /// Machine performance-monitoring event selector.
    Mhpmevent10 = 0x32A,
    /// Machine performance-monitoring event selector.
    Mhpmevent11 = 0x32B,
    /// Machine performance-monitoring event selector.
    Mhpmevent12 = 0x32C,
    /// Machine performance-monitoring event selector.
    Mhpmevent13 = 0x32D,
    /// Machine performance-monitoring event selector.
    Mhpmevent14 = 0x32E,
    /// Machine performance-monitoring event selector.
    Mhpmevent15 = 0x32F,
    /// Machine performance-monitoring event selector.
    Mhpmevent16 = 0x330,
    /// Machine performance-monitoring event selector.
    Mhpmevent17 = 0x331,
    /// Machine performance-monitoring event selector.
    Mhpmevent18 = 0x332,
    /// Machine performance-monitoring event selector.
    Mhpmevent19 = 0x333,
    /// Machine performance-monitoring event selector.
    Mhpmevent20 = 0x334,
    /// Machine performance-monitoring event selector.
    Mhpmevent21 = 0x335,
    /// Machine performance-monitoring event selector.
    Mhpmevent22 = 0x336,
    /// Machine performance-monitoring event selector.
    Mhpmevent23 = 0x337,
    /// Machine performance-monitoring event selector.
    Mhpmevent24 = 0x338,
    /// Machine performance-monitoring event selector.
    Mhpmevent25 = 0x339,
    /// Machine performance-monitoring event selector.
    Mhpmevent26 = 0x33A,
    /// Machine performance-monitoring event selector.
    Mhpmevent27 = 0x33B,
    /// Machine performance-monitoring event selector.
    Mhpmevent28 = 0x33C,
    /// Machine performance-monitoring event selector.
    Mhpmevent29 = 0x33D,
    /// Machine performance-monitoring event selector.
    Mhpmevent30 = 0x33E,
    /// Machine performance-monitoring event selector.
    Mhpmevent31 = 0x33F,

    //
    // Debug/trace registers (`0x7A0..=0x7A3`, `0x7A8`)
    //
    /// Debug/trace trigger register select.
    Tselect = 0x7A0,
    /// First debug/trace trigger data register.
    Tdata1 = 0x7A1,
    /// Second debug/trace trigger data register.
    Tdata2 = 0x7A2,
    /// Third debug/trace trigger data register.
    Tdata3 = 0x7A3,
    /// Machine-mode context register.
    Mcontext = 0x7A8,
}

// impl<U: Into<u16>> TryFrom<U> for Specifier {
//     type Error = InvalidSpecifierError;
//
//     fn try_from(value: U) -> Result<Self, Self::Error> {
//         let value = value.into();
//         if value < 0x1000 {
//             Ok(Self(value))
//         } else {
//             Err(InvalidSpecifierError)
//         }
//     }
// }

impl Specifier {
    /// Returns `true` if this CSR only supports read access.
    pub fn is_read_only(self) -> bool {
        // The top two bits of a CSR specifier indicate whether the CSR is read-only (0b11) or
        // read/write (0b00, 0b01, 0b10)
        self as u16 >> 10 == 0b11
    }

    /// Returns the minimum required privilege level to access this CSR.
    ///
    /// Note that this returns a [`RawPrivilegeLevel`], meaning the minimum required privilege level
    /// may be a reserved level. This still has a defined meaning: only higher privilege levels are
    /// allowed to access the CSR.
    pub fn required_privilege_level(self) -> RawPrivilegeLevel {
        // Bits `10:9` indicate the minimum required privilege level
        RawPrivilegeLevel::from_u2(((self as u16 >> 8) & 0b11) as u8)
    }
}

// /// Attempt to use a specifier that is not in the allowed range of `0..0x1000` (12 bits).
// #[derive(Debug)]
// pub struct InvalidSpecifierError;
//
// impl fmt::Display for InvalidSpecifierError {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         f.write_str("attempted to create specifier from value wider than 12 bits")
//     }
// }
//
// impl std::error::Error for InvalidSpecifierError {}
