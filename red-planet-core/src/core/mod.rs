//! Provides a simulatable RV32I core implementation.

pub mod clint;
mod counter_control;
mod counters;
pub mod csr;
mod envcfg;
mod execute;
mod interrupts;
mod mmu;
mod status;
mod trap;

use crate::core::mmu::MemoryError;
use crate::instruction::{
    AmoOp, BranchCondition, CsrOp, Instruction, LoadWidth, RegImmOp, RegRegOp, RegShiftImmOp,
    StoreWidth,
};
use crate::registers::Registers;
use crate::simulator::Simulatable;
use crate::system_bus::SystemBus;
use crate::{Allocated, Allocator, Endianness, PrivilegeLevel, RawPrivilegeLevel};
use counter_control::CounterControl;
use counters::Counters;
use envcfg::Envcfg;
use execute::Executor;
use interrupts::Interrupts;
use log::{debug, trace};
use mmu::Mmu;
use status::Status;
use std::fmt::Debug;
use thiserror::Error;
use trap::Trap;

pub use csr::CsrSpecifier;

#[derive(Debug, Clone)]
pub struct Config {
    /// > The mhartid CSR is an MXLEN-bit read-only register containing the integer ID of the
    /// > hardware thread running the code. This register must be readable in any implementation.
    /// > Hart IDs might not necessarily be numbered contiguously in a multiprocessor system, but at
    /// > least one hart must have a hart ID of zero. Hart IDs must be unique within the execution
    /// > environment.
    pub hart_id: u32,
    /// Physical memory address of memory-mapped mtime control register.
    /// The register should be 64 bits wide, and the address must support reads of 8 bytes.
    ///
    /// Note that this address is accessed directly on the system bus, ignoring other configuration
    /// options such as [`Config::support_misaligned_memory_access`].
    pub mtime_address: u32,
    /// Physical memory address of memory-mapped mtimecmp control register.
    /// The register should be 64 bits wide, and the address must support reads of 8 bytes.
    ///
    /// Note that this address is accessed directly on the system bus, ignoring other configuration
    /// options such as [`Config::support_misaligned_memory_access`].
    pub mtimecmp_address: u32,
    /// If `true`, non-naturally-aligned memory accesses are supported.
    /// If `false`, they will generate an address-misaligned exception.
    pub support_misaligned_memory_access: bool,
    /// If `true`, instruction must be word-aligned as specified in the spec.
    /// If `false`, instruction must only be haldword-aligned, which would be the default if the C
    /// extension were supported.
    pub strict_instruction_alignment: bool,
    /// Address to which the core's PC register is reset.
    pub reset_vector: u32,
    /// Address of the handler for Non-Maskable Interrupts.
    pub nmi_vector: u32,
}

/// RISC-V core implementing the RV32IMAZicsr ISA.
///
/// As we don't support hardware multithreading, every core always only has a single hart.
/// We therefore don't model RISC-V harts explicitly, but rather consider [`Core`] to be the whole
/// of a core with a single hart.
///
/// > A component is termed a core if it contains an independent instruction fetch unit.
/// > A RISC-V-compatible core might support multiple RISC-V-compatible hardware threads, or harts,
/// > through multithreading.
///
/// # RISC-V hart
///
/// > From the perspective of software running in a given execution environment, a hart is a
/// > resource that autonomously fetches and executes RISC-V instructions within that execution
/// > environment. In this respect, a hart behaves like a hardware thread resource even if
/// > time-multiplexed onto real hardware by the execution environment. Some EEIs support the
/// > creation and destruction of additional harts, for example, via environment calls to fork new
/// > harts.
///
/// > The execution environment is responsible for ensuring the eventual forward progress of each of
/// > its harts. For a given hart, that responsibility is suspended while the hart is exercising a
/// > mechanism that explicitly waits for an event, such as the wait-for-interrupt instruction
/// > defined in Volume II of this specification; and that responsibility ends if the hart is
/// > terminated. The following events constitute forward progress:
/// >
/// > - The retirement of an instruction.
/// > - A trap, as defined in Section 1.6.
/// > - Any other event defined by an extension to constitute forward progress.
///
/// # Control and Status Registers
///
/// This structure also contains the CSRs as per the Zicsr extension.
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
pub struct Core<A: Allocator, B: SystemBus<A>> {
    /// Configuration options for this core. See [`Config`].
    config: Config,
    /// The system bus used via which physical memory is accessed by this core.
    system_bus: B,
    /// General purpose registers: x and pc registers.
    registers: Allocated<A, Registers>,
    /// The core's current privilege mode.
    ///
    /// Allocated separately, because this is updated independently of other registers.
    privilege_mode: Allocated<A, PrivilegeLevel>,
    /// Status (mstatus, mstatush, sstatus) registers.
    ///
    /// Allocated separately, because these are often mutated independently of other registers.
    status: Allocated<A, Status>,
    /// All counter registers.
    ///
    /// Allocated together, since most of them will be updated simultaneously.
    counters: Allocated<A, Counters>,
    /// Counter control registers (mcounteren, mcountinhibit, scounteren).
    ///
    /// Allocated separately from counters, since they will likely not be updated as frequently.
    counter_control: Allocated<A, CounterControl>,
    /// Trap-related registers.
    ///
    /// Allocated together, because they are most often all written when taking a trap, or returning
    /// from one.
    trap: Allocated<A, Trap>,
    /// Interrupt (mip, mie, sip, sie) registers.
    ///
    /// Allocated together, because they are most often accessed together.
    interrupts: Allocated<A, Interrupts>,
    /// Envcfg (menvcfg, menvcfgh, senvcfg) registers.
    ///
    /// Allocated separately, because these are mutated independently of other registers, and likely
    /// not used often.
    envcfg: Allocated<A, Envcfg>,
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    /// The misa CSR is set to `0x4014_1101`, indicating that MXL=32 and that the following
    /// extensions are supported: A, I, M, S, U.
    ///
    /// > The misa CSR is a WARL read-write register reporting the ISA supported by the hart. This
    /// > register must be readable in any implementation, but a value of zero can be returned to
    /// > indicate the misa register has not been implemented, requiring that CPU capabilities be
    /// > determined through a separate non-standard mechanism.
    ///
    /// > The MXL (Machine XLEN) field encodes the native base integer ISA width as shown in Table
    /// > 3.1. The MXL field may be writable in implementations that support multiple base ISAs.
    /// > The effective XLEN in M-mode, MXLEN, is given by the setting of MXL, or has a fixed value
    /// > if misa is zero. The MXL field is always set to the widest supported ISA variant at reset.
    ///
    /// > Table 3.1: Encoding of MXL field in misa.
    /// > | MXL | XLEN |
    /// > | ---:| ----:|
    /// > |   1 |   32 |
    /// > |   2 |   64 |
    /// > |   3 |  128 |
    pub const MISA: u32 = 0x4014_1101;
    /// The mvendorid CSR is set to 0 to indicate this is a non-commercial implementation.
    ///
    /// > The mvendorid CSR is a 32-bit read-only register providing the JEDEC manufacturer ID of
    /// > the provider of the core. This register must be readable in any implementation, but a
    /// > value of 0 can be returned to indicate the field is not implemented or that this is a
    /// > non-commercial implementation.
    pub const MVENDORID: u32 = 0;
    /// The marchid CSR is set to 0 to indicate it is not implemented.
    ///
    /// > The marchid CSR is an MXLEN-bit read-only register encoding the base microarchitecture of
    /// > the hart. This register must be readable in any implementation, but a value of 0 can be
    /// > returned to indicate the field is not implemented. The combination of mvendorid and
    /// > marchid should uniquely identify the type of hart microarchitecture that is implemented.
    pub const MARCHID: u32 = 0;
    /// The mimpid CSR is set to 0 to indicate it is not implemented.
    ///
    /// > The mimpid CSR provides a unique encoding of the version of the processor implementation.
    /// > This register must be readable in any implementation, but a value of 0 can be returned to
    /// > indicate that the field is not implemented. The Implementation value should reflect the
    /// > design of the RISC-V processor itself and not any surrounding system.
    pub const MIMPID: u32 = 0;
    /// The mconfigptr CSR is set to 0 to indicate the configuration data structure does not exists.
    ///
    /// > mconfigptr is an MXLEN-bit read-only CSR [...] that holds the physical address of a
    /// > configuration data structure. Software can traverse this data structure to discover
    /// > information about the harts, the platform, and their configuration.
    pub const MCONFIGPTR: u32 = 0;

    pub fn new(allocator: &mut A, system_bus: B, config: Config) -> Self {
        debug!("Creating core with config {config:?}");
        let registers = Allocated::new(allocator, Registers::new(config.reset_vector));
        Self {
            config,
            system_bus,
            registers,
            privilege_mode: Allocated::new(allocator, PrivilegeLevel::Machine),
            status: Allocated::new(allocator, Status::new()),
            counters: Allocated::new(allocator, Counters::new()),
            counter_control: Allocated::new(allocator, CounterControl::new()),
            trap: Allocated::new(allocator, Trap::new()),
            interrupts: Allocated::new(allocator, Interrupts::new()),
            envcfg: Allocated::new(allocator, Envcfg::new()),
        }
    }

    pub fn drop(self, allocator: &mut A) {
        self.registers.drop(allocator);
        self.privilege_mode.drop(allocator);
        self.status.drop(allocator);
        self.counters.drop(allocator);
        self.counter_control.drop(allocator);
        self.trap.drop(allocator);
        self.interrupts.drop(allocator);
        self.envcfg.drop(allocator);
    }

    pub fn system_bus(&self) -> &B {
        &self.system_bus
    }

    /// Provide a read-only view of this core's configuration.
    ///
    /// It is not possible to modify the configuration after creation.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the Hart ID that was assigned to this core's single Hart.
    pub fn hart_id(&self) -> u32 {
        self.config.hart_id
    }

    /// Returns the current privilege mode.
    ///
    /// Note that loads and stores execute at the
    /// [`effective_privilege_mode`](Self::effective_privilege_mode).
    ///
    /// See also [`PrivilegeLevel`].
    pub fn privilege_mode(&self, allocator: &A) -> PrivilegeLevel {
        *self.privilege_mode.get(allocator)
    }

    /// Returns the current *effective privilege mode*. This is the privilege level at which load
    /// and stores execute (but not instruction fetches).
    ///
    /// See [`privilege_mode`](Self::privilege_mode) for the privilege mode used for all other
    /// operations.
    ///
    /// See also [`PrivilegeLevel`].
    pub fn effective_privilege_mode(&self, allocator: &A) -> PrivilegeLevel {
        let status = self.status.get(allocator);
        match status.mprv() {
            true => status.mpp(),
            false => *self.privilege_mode.get(allocator),
        }
    }

    /// Returns the endianness of the core for the given privilege mode.
    pub fn endianness(&self, allocator: &A, privilege_mode: PrivilegeLevel) -> Endianness {
        let status = self.status.get(allocator);
        let be = match privilege_mode {
            PrivilegeLevel::User => status.ube(),
            PrivilegeLevel::Supervisor => status.sbe(),
            PrivilegeLevel::Machine => status.mbe(),
        };
        match be {
            true => Endianness::BE,
            false => Endianness::LE,
        }
    }

    /// Provides immutable access to the general purpose (x) registers, and the pc register.
    pub fn registers<'a>(&self, allocator: &'a A) -> &'a Registers {
        self.registers.get(allocator)
    }

    /// Provides mutable access to the general purpose (x) registers, and the pc register.
    pub fn registers_mut<'a>(&self, allocator: &'a mut A) -> &'a mut Registers {
        self.registers.get_mut(allocator)
    }

    /// Generate a Reset.
    pub fn reset(&self, allocator: &mut A) {
        trace!("Resetting core");
        // Clear all x registers, reset pc to the configured reset vector.
        *self.registers.get_mut(allocator) = Registers::new(self.config.reset_vector);
        // Set mcause to an all-zero value.
        let trap = self.trap.get_mut(allocator);
        trap.set_m_trap_cause(None::<Exception>);
        // Reset all counters.
        *self.counters.get_mut(allocator) = Counters::new();
        // Clear relevant bits in status registers.
        let status = self.status.get_mut(allocator);
        status.set_mie(false);
        status.set_mprv(false);
        status.set_mbe(false);
        // Switch to M-mode.
        *self.privilege_mode.get_mut(allocator) = PrivilegeLevel::Machine;
        // Reset control registers.
        *self.counter_control.get_mut(allocator) = CounterControl::new();
        // Reset mconfig register.
        *self.envcfg.get_mut(allocator) = Envcfg::new();
    }

    /// Generate a Non-Maskable Interrupt.
    pub fn nmi(&self, allocator: &mut A) {
        trace!("Taking Non-Maskable Interrupt in core");
        // NMIs do not reset state; they only update a few registers.
        // Jump to configured nmi vector.
        let pc = self.registers.get_mut(allocator).pc_mut();
        let old_pc = std::mem::replace(pc, self.config.nmi_vector);
        // Set mepc and mcause registers appropriately.
        let trap = self.trap.get_mut(allocator);
        trap.set_mepc(old_pc);
        trap.set_m_trap_cause(None::<Interrupt>);
        // Switch to M-mode.
        *self.privilege_mode.get_mut(allocator) = PrivilegeLevel::Machine;
    }

    /// Read the value of a CSR by its specifier.
    ///
    /// `privilege_level` indicates at what privilege level the read is performed. If the CSR that
    /// is being read requires a higher privilege level (see
    /// [`csr::required_privilege_level`]), then an [`CsrAccessError::Privileged`] will be
    /// given.
    pub fn read_csr(
        &self,
        allocator: &mut A,
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
    ) -> CsrReadResult {
        trace!("Reading CSR {specifier} at privilege level {privilege_level}");
        self.check_csr_access(allocator, specifier, privilege_level)?;
        // Ordered according to CSR Listing in the privileged spec.
        match specifier {
            //
            // Unprivileged Floating-Point CSRs
            //
            csr::FFLAGS | csr::FRM | csr::FCSR => Err(CsrAccessError::CsrUnsupported(specifier)),
            //
            // Unprivileged Counter/Timers
            //
            csr::CYCLE => self.read_cycle(allocator),
            csr::TIME => self.read_time(allocator),
            csr::INSTRET => self.read_instret(allocator),
            csr::HPMCOUNTER3..=csr::HPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::HPMCOUNTER3);
                self.read_hpmcounter(allocator, offset as u8)
            }
            csr::CYCLEH => self.read_cycleh(allocator),
            csr::TIMEH => self.read_timeh(allocator),
            csr::INSTRETH => self.read_instreth(allocator),
            csr::HPMCOUNTER3H..=csr::HPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::HPMCOUNTER3H);
                self.read_hpmcounterh(allocator, offset as u8)
            }
            //
            // Supervisor Trap Setup
            //
            csr::SSTATUS => self.read_sstatus(allocator),
            csr::SIE => self.read_sie(allocator),
            csr::STVEC => self.read_stvec(allocator),
            csr::SCOUNTEREN => self.read_scounteren(allocator),
            //
            // Supervisor Configuration
            //
            csr::SENVCFG => self.read_menvcfg(allocator),
            //
            // Supervisor Trap Handling
            //
            csr::SSCRATCH => self.read_sscratch(allocator),
            csr::SEPC => self.read_sepc(allocator),
            csr::SCAUSE => self.read_scause(allocator),
            csr::STVAL => self.read_stval(allocator),
            csr::SIP => self.read_sip(allocator),
            //
            // Supervisor Protection and Translation
            //
            csr::SATP => self.read_satp(allocator),
            //
            // Machine Information Registers
            //
            csr::MVENDORID => Ok(Self::MVENDORID),
            csr::MARCHID => Ok(Self::MARCHID),
            csr::MIMPID => Ok(Self::MIMPID),
            csr::MHARTID => Ok(self.config.hart_id),
            csr::MCONFIGPTR => Ok(Self::MCONFIGPTR),
            //
            // Machine Trap Setup
            //
            csr::MSTATUS => self.read_mstatus(allocator),
            csr::MISA => Ok(Self::MISA),
            csr::MEDELEG => self.read_medeleg(allocator),
            csr::MIDELEG => self.read_mideleg(allocator),
            csr::MIE => self.read_mie(allocator),
            csr::MTVEC => self.read_mtvec(allocator),
            csr::MCOUNTEREN => self.read_mcounteren(allocator),
            csr::MSTATUSH => self.read_mstatush(allocator),
            //
            // Machine Trap Handling
            //
            csr::MSCRATCH => self.read_mscratch(allocator),
            csr::MEPC => self.read_mepc(allocator),
            csr::MCAUSE => self.read_mcause(allocator),
            csr::MTVAL => self.read_mtval(allocator),
            csr::MIP => self.read_mip(allocator),
            csr::MTINST => self.read_mtinst(allocator),
            csr::MTVAL2 => self.read_mtval2(allocator),
            //
            // Machine Configuration
            //
            csr::MENVCFG => self.read_menvcfg(allocator),
            csr::MENVCFGH => self.read_menvcfgh(allocator),
            csr::MSECCFG | csr::MSECCFGH => Err(CsrAccessError::CsrUnsupported(specifier)),
            //
            // Machine Memory Protection
            //
            csr::PMPCFG0..=csr::PMPCFG15 | csr::PMPADDR0..=csr::PMPADDR63 => {
                Err(CsrAccessError::CsrUnsupported(specifier))
            }
            //
            // Machine Counters/Timers
            //
            csr::MCYCLE => self.read_mcycle(allocator),
            csr::MINSTRET => self.read_minstret(allocator),
            csr::MHPMCOUNTER3..=csr::MHPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3);
                self.read_mhpmcounter(allocator, offset as u8)
            }
            csr::MCYCLEH => self.read_mcycleh(allocator),
            csr::MINSTRETH => self.read_minstreth(allocator),
            csr::MHPMCOUNTER3H..=csr::MHPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3H);
                self.read_mhpmcounterh(allocator, offset as u8)
            }
            //
            // Machine Counter Setup
            //
            csr::MCOUNTINHIBIT => self.read_mcountinhibit(allocator),
            csr::MHPMEVENT3..=csr::MHPMEVENT31 => {
                let offset = 3 + (specifier - csr::MHPMEVENT3);
                self.read_mhpmevent(allocator, offset as u8)
            }
            //
            // Debug/Trace Registers
            //
            csr::TSELECT | csr::TDATA1 | csr::TDATA2 | csr::TDATA3 | csr::MCONTEXT => {
                Err(CsrAccessError::CsrUnsupported(specifier))
            }
            _ => Err(CsrAccessError::CsrUnsupported(specifier)),
        }
    }

    /// Write a (masked) value to a CSR by its specifier.
    ///
    /// `privilege_level` indicates at what privilege level the write is performed. If the CSR that
    /// is being written requires a higher privilege level (see
    /// [`csr::required_privilege_level`]), then an [`CsrAccessError::Privileged`] will be
    /// given.
    ///
    /// Only the bits of `value` for which the corresponding bit in `mask` is `1` will be written.
    /// However, even if `mask == 0`, write side-effects will still be performed.
    pub fn write_csr(
        &self,
        allocator: &mut A,
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
        value: u32,
        mask: u32,
    ) -> CsrWriteResult {
        trace!(value, mask; "Writing CSR {specifier} at privilege level {privilege_level}");
        self.check_csr_access(allocator, specifier, privilege_level)?;
        if csr::is_read_only(specifier) {
            return Err(CsrWriteError::WriteToReadOnly);
        }
        // Ordered according to CSR Listing in the privileged spec.
        match specifier {
            //
            // Unprivileged Floating-Point CSRs
            //
            csr::FFLAGS | csr::FRM | csr::FCSR => Err(CsrAccessError::CsrUnsupported(specifier))?,
            //
            // Unprivileged Counter/Timers (read-only)
            //
            csr::CYCLE
            | csr::TIME
            | csr::INSTRET
            | csr::HPMCOUNTER3..=csr::HPMCOUNTER31
            | csr::CYCLEH
            | csr::TIMEH
            | csr::INSTRETH
            | csr::HPMCOUNTER3H..=csr::HPMCOUNTER31H => unreachable!(),
            //
            // Supervisor Trap Setup
            //
            csr::SSTATUS => self.write_sstatus(allocator, value, mask),
            csr::SIE => self.write_sie(allocator, value, mask),
            csr::STVEC => self.write_stvec(allocator, value, mask),
            csr::SCOUNTEREN => self.write_scounteren(allocator, value, mask),
            //
            // Supervisor Configuration
            //
            csr::SENVCFG => self.write_menvcfg(allocator, value, mask),
            //
            // Supervisor Trap Handling
            //
            csr::SSCRATCH => self.write_sscratch(allocator, value, mask),
            csr::SEPC => self.write_sepc(allocator, value, mask),
            csr::SCAUSE => self.write_scause(allocator, value, mask),
            csr::STVAL => self.write_stval(allocator, value, mask),
            csr::SIP => self.write_sip(allocator, value, mask),
            //
            // Supervisor Protection and Translation
            //
            csr::SATP => self.write_satp(allocator, value, mask),
            //
            // Machine Information Registers (read-only)
            //
            csr::MVENDORID | csr::MARCHID | csr::MIMPID | csr::MHARTID | csr::MCONFIGPTR => {
                unreachable!()
            }
            //
            // Machine Trap Setup
            //
            csr::MSTATUS => self.write_mstatus(allocator, value, mask),
            csr::MISA => Ok(()),
            csr::MEDELEG => self.write_medeleg(allocator, value, mask),
            csr::MIDELEG => self.write_mideleg(allocator, value, mask),
            csr::MIE => self.write_mie(allocator, value, mask),
            csr::MTVEC => self.write_mtvec(allocator, value, mask),
            csr::MCOUNTEREN => self.write_mcounteren(allocator, value, mask),
            csr::MSTATUSH => self.write_mstatush(allocator, value, mask),
            //
            // Machine Trap Handling
            //
            csr::MSCRATCH => self.write_mscratch(allocator, value, mask),
            csr::MEPC => self.write_mepc(allocator, value, mask),
            csr::MCAUSE => self.write_mcause(allocator, value, mask),
            csr::MTVAL => self.write_mtval(allocator, value, mask),
            csr::MIP => self.write_mip(allocator, value, mask),
            csr::MTINST => self.write_mtinst(allocator, value, mask),
            csr::MTVAL2 => self.write_mtval2(allocator, value, mask),
            //
            // Machine Configuration
            //
            csr::MENVCFG => self.write_menvcfg(allocator, value, mask),
            csr::MENVCFGH => self.write_menvcfgh(allocator, value, mask),
            csr::MSECCFG | csr::MSECCFGH => Err(CsrAccessError::CsrUnsupported(specifier))?,
            //
            // Machine Memory Protection
            //
            csr::PMPCFG0..=csr::PMPCFG15 | csr::PMPADDR0..=csr::PMPADDR63 => {
                Err(CsrAccessError::CsrUnsupported(specifier))?
            }
            //
            // Machine Counters/Timers
            //
            csr::MCYCLE => self.write_mcycle(allocator, value, mask),
            csr::MINSTRET => self.write_minstret(allocator, value, mask),
            csr::MHPMCOUNTER3..=csr::MHPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3);
                self.write_mhpmcounter(allocator, offset as u8, value, mask)
            }
            csr::MCYCLEH => self.write_mcycleh(allocator, value, mask),
            csr::MINSTRETH => self.write_minstreth(allocator, value, mask),
            csr::MHPMCOUNTER3H..=csr::MHPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3H);
                self.write_mhpmcounterh(allocator, offset as u8, value, mask)
            }
            //
            // Machine Counter Setup
            //
            csr::MCOUNTINHIBIT => self.write_mcountinhibit(allocator, value, mask),
            csr::MHPMEVENT3..=csr::MHPMEVENT31 => {
                let offset = 3 + (specifier - csr::MHPMEVENT3);
                self.write_mhpmevent(allocator, offset as u8, value, mask)
            }
            //
            // Debug/Trace Registers
            //
            csr::TSELECT | csr::TDATA1 | csr::TDATA2 | csr::TDATA3 | csr::MCONTEXT => {
                Err(CsrAccessError::CsrUnsupported(specifier))?
            }
            _ => Err(CsrAccessError::CsrUnsupported(specifier))?,
        }
    }

    fn check_csr_access(
        &self,
        _allocator: &A,
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
    ) -> Result<(), CsrAccessError> {
        if !csr::is_valid(specifier) {
            debug!("Attempt to access unsupported CSR {specifier}");
            return Err(CsrAccessError::CsrUnsupported(specifier));
        }
        let required_level = csr::required_privilege_level(specifier);
        if privilege_level < required_level {
            debug!(
                "Attempt to access CSR {specifier} at insufficient privilege level \
                 {privilege_level} (requires {required_level})"
            );
            return Err(CsrAccessError::Privileged {
                specifier,
                required_level,
                actual_level: privilege_level,
            });
        }
        Ok(())
    }

    /// Performs a read of the memory-mapped mtime CSR.
    pub fn read_mtime(&self, allocator: &mut A) -> u64 {
        let mut buf = [0u8; 8];
        self.system_bus
            .read(&mut buf, allocator, self.config.mtime_address);
        u64::from_le_bytes(buf)
    }

    /// Performs a read of the memory-mapped mtimecmp CSR.
    pub fn read_mtimecmp(&self, allocator: &mut A) -> u64 {
        let mut buf = [0u8; 8];
        self.system_bus
            .read(&mut buf, allocator, self.config.mtimecmp_address);
        u64::from_le_bytes(buf)
    }

    /// Provides an access wrapper around the system bus to address it as memory from this core's
    /// point of view.
    ///
    /// This takes into account the core's current privilege level, its memory mapping (i.e. which
    /// regions can be accessed), its configuration (e.g. whether misaligned memory accesses are
    /// supported), etc.
    pub fn mmu(&self) -> Mmu<A, B> {
        Mmu { core: self }
    }

    /// Fetch the next instruction at pc and execute.
    ///
    /// If an interrupt is ready to be taken, this will perform a trap for that interrupt, rather
    /// than executing the next instruction. Additionally, if the executed instruction causes an
    /// interrupt (indirectly), it will also be taken by this method.
    pub fn step(&self, allocator: &mut A) {
        let pc = self.registers(allocator).pc();
        trace!("Stepping core, pc = {pc:#010x}");
        if self.check_for_interrupts(allocator) {
            return;
        }
        let raw_instruction =
            self.mmu()
                .fetch_instruction(allocator, pc)
                .map_err(|err| match err {
                    MemoryError::MisalignedAccess => Exception::InstructionAddressMisaligned(pc),
                    MemoryError::AccessFault => Exception::InstructionAccessFault(pc),
                    MemoryError::PageFault => Exception::InstructionPageFault(pc),
                });
        self.step_with_raw(allocator, raw_instruction);
        self.check_for_interrupts(allocator);
    }

    /// Execute a single (raw) instruction.
    ///
    /// Never checks for interrupts.
    fn step_with_raw(&self, allocator: &mut A, raw_instruction: ExecutionResult<u32>) {
        let instruction = raw_instruction.and_then(|raw| {
            Instruction::decode(raw).map_err(|_| Exception::IllegalInstruction(Some(raw)))
        });
        self.step_with(allocator, instruction);
    }

    /// Execute a single (decoded) instruction.
    ///
    /// Never checks for interrupts.
    fn step_with(&self, allocator: &mut A, instruction: ExecutionResult<Instruction>) {
        let exception = instruction
            .and_then(|instruction| self.execute_instruction(allocator, instruction))
            .err();

        if let Some(exception) = exception {
            trace!("Executing instruction caused exception {exception:?}");
        }

        trace!("Updating counters after instruction execution");
        self.increment_cycle_counter(allocator);
        match instruction {
            // ECALL and EBREAK are not considered to retire.
            // Similarly, if the instruction fetch failed, then instret should not be incremented.
            Ok(Instruction::Ecall | Instruction::Ebreak) | Err(_) => {}
            _ => self.increment_instret_counter(allocator),
        };

        if let Some(exception) = exception {
            self.trap(allocator, exception.into());
        }
    }

    /// Execute a single (raw) instruction.
    ///
    /// This is not the same as [`Self::step`]! This only takes care of executing the
    /// instruction-specific operations, such as updating `x` registers, updating memory, updating
    /// the `pc` register, and depending on the instruction also updating CSRs. However, additional
    /// state updates that normally happen at a tick, such as incrementing the appropriate counters,
    /// are not performed. This does also not check for interrupts that are ready.
    ///
    /// This can be useful for executing the operation defined by an instruction, without actually
    /// progressing general execution. If used for this scenario, consider first decrementing the
    /// `pc` register by `4` so that the current instruction is in fact treated as the next, which
    /// will ensure the `pc` register will be as expected after executing the instruction. Take into
    /// account that this influences jump/branch targets.
    ///
    /// # Unspecified behavior
    ///
    /// > The behavior upon decoding a reserved instruction is UNSPECIFIED.
    ///
    /// This implementation chooses to raise an [`Exception::IllegalInstruction`] when
    /// `raw_instruction` has a reserved opcode.
    pub fn execute_raw_instruction(
        &self,
        allocator: &mut A,
        raw_instruction: u32,
    ) -> ExecutionResult {
        let instruction = Instruction::decode(raw_instruction)
            .map_err(|_| Exception::IllegalInstruction(Some(raw_instruction)))?;
        self.execute_instruction(allocator, instruction)
            .map_err(|err| match err {
                Exception::IllegalInstruction(None) => {
                    Exception::IllegalInstruction(Some(raw_instruction))
                }
                err => err,
            })
    }

    /// Execute a single (decoded) instruction.
    ///
    /// Performs the same operation as [`Self::execute_raw_instruction`], but takes an already
    /// decoded instruction.
    ///
    /// Note that this is not the same as [`Self::step`]!
    /// See [`Self::execute_raw_instruction`] for why.
    pub fn execute_instruction(
        &self,
        allocator: &mut A,
        instruction: Instruction,
    ) -> ExecutionResult {
        trace!("Executing instruction {instruction:?}");
        let mut executor = Executor {
            allocator,
            core: self,
        };
        match instruction {
            Instruction::OpImm {
                op,
                dest,
                src,
                immediate,
            } => {
                let op = match op {
                    RegImmOp::Addi => Executor::addi,
                    RegImmOp::Slti => Executor::slti,
                    RegImmOp::Sltiu => Executor::sltiu,
                    RegImmOp::Xori => Executor::xori,
                    RegImmOp::Ori => Executor::ori,
                    RegImmOp::Andi => Executor::andi,
                };
                op(&mut executor, dest, src, immediate)
            }
            Instruction::OpShiftImm {
                op,
                dest,
                src,
                shift_amount_u5,
            } => {
                let op = match op {
                    RegShiftImmOp::Slli => Executor::slli,
                    RegShiftImmOp::Srli => Executor::srli,
                    RegShiftImmOp::Srai => Executor::srai,
                };
                op(&mut executor, dest, src, shift_amount_u5)
            }
            Instruction::Auipc { dest, immediate } => executor.auipc(dest, immediate),
            Instruction::Lui { dest, immediate } => executor.lui(dest, immediate),
            Instruction::Amo {
                op,
                aq: _,
                rl: _,
                src,
                addr,
                dest,
            } => {
                let op = match op {
                    AmoOp::Lr => Executor::lr,
                    AmoOp::Sc => Executor::sc,
                    AmoOp::Swap => Executor::amoswap,
                    AmoOp::Add => Executor::amoadd,
                    AmoOp::Xor => Executor::amoxor,
                    AmoOp::And => Executor::amoand,
                    AmoOp::Or => Executor::amoor,
                    AmoOp::Min => Executor::amomin,
                    AmoOp::Max => Executor::amomax,
                    AmoOp::Minu => Executor::amominu,
                    AmoOp::Maxu => Executor::amomaxu,
                };
                op(&mut executor, dest, src, addr)
            }
            Instruction::Op {
                op,
                dest,
                src1,
                src2,
            } => {
                let op = match op {
                    RegRegOp::Add => Executor::add,
                    RegRegOp::Slt => Executor::slt,
                    RegRegOp::Sltu => Executor::sltu,
                    RegRegOp::And => Executor::and,
                    RegRegOp::Or => Executor::or,
                    RegRegOp::Xor => Executor::xor,
                    RegRegOp::Sll => Executor::sll,
                    RegRegOp::Srl => Executor::srl,
                    RegRegOp::Sub => Executor::sub,
                    RegRegOp::Sra => Executor::sra,
                    RegRegOp::Mul => Executor::mul,
                    RegRegOp::Mulh => Executor::mulh,
                    RegRegOp::Mulhsu => Executor::mulhsu,
                    RegRegOp::Mulhu => Executor::mulhu,
                    RegRegOp::Div => Executor::div,
                    RegRegOp::Divu => Executor::divu,
                    RegRegOp::Rem => Executor::rem,
                    RegRegOp::Remu => Executor::remu,
                };
                op(&mut executor, dest, src1, src2)
            }
            Instruction::Jal { dest, offset } => executor.jal(dest, offset),
            Instruction::Jalr { dest, base, offset } => executor.jalr(dest, base, offset),
            Instruction::Branch {
                condition,
                src1,
                src2,
                offset,
            } => {
                let op = match condition {
                    BranchCondition::Beq => Executor::beq,
                    BranchCondition::Bne => Executor::bne,
                    BranchCondition::Blt => Executor::blt,
                    BranchCondition::Bltu => Executor::bltu,
                    BranchCondition::Bge => Executor::bge,
                    BranchCondition::Bgeu => Executor::bgeu,
                };
                op(&mut executor, src1, src2, offset)
            }
            Instruction::Load {
                width,
                dest,
                base,
                offset,
            } => {
                let op = match width {
                    LoadWidth::Lb => Executor::lb,
                    LoadWidth::Lh => Executor::lh,
                    LoadWidth::Lw => Executor::lw,
                    LoadWidth::Lbu => Executor::lbu,
                    LoadWidth::Lhu => Executor::lhu,
                };
                op(&mut executor, dest, base, offset)
            }
            Instruction::Store {
                width,
                src,
                base,
                offset,
            } => {
                let op = match width {
                    StoreWidth::Sb => Executor::sb,
                    StoreWidth::Sh => Executor::sh,
                    StoreWidth::Sw => Executor::sw,
                };
                op(&mut executor, src, base, offset)
            }
            Instruction::Fence {
                predecessor,
                successor,
            } => executor.fence(predecessor, successor),
            Instruction::Ecall => executor.ecall(),
            Instruction::Ebreak => executor.ebreak(),
            Instruction::Sret => executor.sret(),
            Instruction::Mret => executor.mret(),
            Instruction::Wfi => executor.wfi(),
            Instruction::SfenceVma { vaddr, asid } => executor.sfence_vma(vaddr, asid),
            Instruction::Csr { op, dest, csr, src } => {
                let op = match op {
                    CsrOp::ReadWrite => Executor::csrrw,
                    CsrOp::ReadSet => Executor::csrrs,
                    CsrOp::ReadClear => Executor::csrrc,
                };
                op(&mut executor, dest, csr, src)
            }
            Instruction::Csri {
                op,
                dest,
                csr,
                immediate,
            } => {
                let op = match op {
                    CsrOp::ReadWrite => Executor::csrrwi,
                    CsrOp::ReadSet => Executor::csrrsi,
                    CsrOp::ReadClear => Executor::csrrci,
                };
                op(&mut executor, dest, csr, immediate)
            }
        }
    }

    /// Checks whether any interrupts are pending and can be taken. If so, it executes the
    /// appopriate trap logic, and returns `true`. If not, it just returns `false`.
    fn check_for_interrupts(&self, allocator: &mut A) -> bool {
        trace!("Checking for interrupts");
        match self.highest_priority_ready_interrupt(allocator) {
            Some(interrupt) => {
                debug!("Found ready interrupt {interrupt:?}, taking it");
                self.trap(allocator, interrupt.into());
                true
            }
            None => {
                trace!("No interrupts ready");
                false
            }
        }
    }
}

impl<A: Allocator, B: SystemBus<A>> Simulatable<A> for Core<A, B> {
    fn tick(&self, allocator: &mut A) {
        self.step(allocator)
    }

    fn drop(self, allocator: &mut A) {
        self.drop(allocator)
    }
}

pub type CsrReadResult<T = u32> = Result<T, CsrAccessError>;
pub type CsrWriteResult<T = ()> = Result<T, CsrWriteError>;

/// Errors that can occur when attempting to access a CSR.
#[derive(Error, Debug)]
pub enum CsrAccessError {
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
    #[error("CSR {0:#05X} unavailable: {0}")]
    CsrUnavailable(CsrSpecifier, String),
}

/// Errors that can occur when attempting to write to a CSR.
#[derive(Error, Debug)]
pub enum CsrWriteError {
    /// A non-write specific access error. See [`CsrAccessError`].
    #[error("{0}")]
    AccessError(CsrAccessError),
    /// Attempt to write to a read-only register.
    #[error("writing to read-only CSR is invalid")]
    WriteToReadOnly,
}

impl From<CsrAccessError> for CsrWriteError {
    fn from(value: CsrAccessError) -> Self {
        Self::AccessError(value)
    }
}

/// Result of executing a single instruction. [`Ok`] if execution went normal, [`Err`] if an
/// exception occurred.
pub type ExecutionResult<T = ()> = Result<T, Exception>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionCode {
    InstructionAddressMisaligned = 0,
    InstructionAccessFault = 1,
    IllegalInstruction = 2,
    Breakpoint = 3,
    LoadAddressMisaligned = 4,
    LoadAccessFault = 5,
    StoreOrAmoAddressMisaligned = 6,
    StoreOrAmoAccessFault = 7,
    EnvironmentCallFromUMode = 8,
    EnvironmentCallFromSMode = 9,
    EnvironmentCallFromMMode = 11,
    InstructionPageFault = 12,
    LoadPageFault = 13,
    StoreOrAmoPageFault = 15,
}

impl ExceptionCode {
    pub const INSTRUCTION_ADDRESS_MISALIGNED: u32 = Self::InstructionAddressMisaligned as u32;
    pub const INSTRUCTION_ACCESS_FAULT: u32 = Self::InstructionAccessFault as u32;
    pub const ILLEGAL_INSTRUCTION: u32 = Self::IllegalInstruction as u32;
    pub const BREAKPOINT: u32 = Self::Breakpoint as u32;
    pub const LOAD_ADDRESS_MISALIGNED: u32 = Self::LoadAddressMisaligned as u32;
    pub const LOAD_ACCESS_FAULT: u32 = Self::LoadAccessFault as u32;
    pub const STORE_OR_AMO_ADDRESS_MISALIGNED: u32 = Self::StoreOrAmoAddressMisaligned as u32;
    pub const STORE_OR_AMO_ACCESS_FAULT: u32 = Self::StoreOrAmoAccessFault as u32;
    pub const ENVIRONMENT_CALL_FROM_U_MODE: u32 = Self::EnvironmentCallFromUMode as u32;
    pub const ENVIRONMENT_CALL_FROM_S_MODE: u32 = Self::EnvironmentCallFromSMode as u32;
    pub const ENVIRONMENT_CALL_FROM_M_MODE: u32 = Self::EnvironmentCallFromMMode as u32;
    pub const INSTRUCTION_PAGE_FAULT: u32 = Self::InstructionPageFault as u32;
    pub const LOAD_PAGE_FAULT: u32 = Self::LoadPageFault as u32;
    pub const STORE_OR_AMO_PAGE_FAULT: u32 = Self::StoreOrAmoPageFault as u32;
}

impl TryFrom<u32> for ExceptionCode {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            Self::INSTRUCTION_ADDRESS_MISALIGNED => Ok(Self::InstructionAddressMisaligned),
            Self::INSTRUCTION_ACCESS_FAULT => Ok(Self::InstructionAccessFault),
            Self::ILLEGAL_INSTRUCTION => Ok(Self::IllegalInstruction),
            Self::BREAKPOINT => Ok(Self::Breakpoint),
            Self::LOAD_ADDRESS_MISALIGNED => Ok(Self::LoadAddressMisaligned),
            Self::LOAD_ACCESS_FAULT => Ok(Self::LoadAccessFault),
            Self::STORE_OR_AMO_ADDRESS_MISALIGNED => Ok(Self::StoreOrAmoAddressMisaligned),
            Self::STORE_OR_AMO_ACCESS_FAULT => Ok(Self::StoreOrAmoAccessFault),
            Self::ENVIRONMENT_CALL_FROM_U_MODE => Ok(Self::EnvironmentCallFromUMode),
            Self::ENVIRONMENT_CALL_FROM_S_MODE => Ok(Self::EnvironmentCallFromSMode),
            Self::ENVIRONMENT_CALL_FROM_M_MODE => Ok(Self::EnvironmentCallFromMMode),
            Self::INSTRUCTION_PAGE_FAULT => Ok(Self::InstructionPageFault),
            Self::LOAD_PAGE_FAULT => Ok(Self::LoadPageFault),
            Self::STORE_OR_AMO_PAGE_FAULT => Ok(Self::StoreOrAmoPageFault),
            _ => Err(format!("unsupported exception code: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Exception {
    /// Instruction address is not on a four-byte aligned boundary in memory.
    ///
    /// The inner value is the faulting virtual address.
    InstructionAddressMisaligned(u32),
    /// The inner value is the faulting virtual address.
    InstructionAccessFault(u32),
    /// Generic exception used to communicate one of many possible scenarios:
    ///
    /// - (*UNSPECIFIED*) Attempt to decode a reserved instruction.
    /// - Attempt to access a non-existent CSR.
    /// - Attempt to access a CSR without the appropriate privilege level.
    /// - Attempt to write to a read-only CSR.
    ///
    /// The inner value is the raw instruction if that data was available.
    IllegalInstruction(Option<u32>),
    Breakpoint,
    /// The inner value is the virtual address of the portion of the access that caused the fault.
    LoadAddressMisaligned(u32),
    /// The inner value is the faulting virtual address.
    LoadAccessFault(u32),
    /// The inner value is the virtual address of the portion of the access that caused the fault.
    StoreOrAmoAddressMisaligned(u32),
    /// The inner value is the faulting virtual address.
    StoreOrAmoAccessFault(u32),
    EnvironmentCallFromUMode,
    EnvironmentCallFromSMode,
    EnvironmentCallFromMMode,
    /// The inner value is the faulting virtual address.
    InstructionPageFault(u32),
    /// The inner value is the faulting virtual address.
    LoadPageFault(u32),
    /// The inner value is the faulting virtual address.
    StoreOrAmoPageFault(u32),
}

impl Exception {
    /// Returns the exception code (cause) for this exception.
    pub const fn code(&self) -> ExceptionCode {
        match self {
            Self::InstructionAddressMisaligned(_) => ExceptionCode::InstructionAddressMisaligned,
            Self::InstructionAccessFault(_) => ExceptionCode::InstructionAccessFault,
            Self::IllegalInstruction(_) => ExceptionCode::IllegalInstruction,
            Self::Breakpoint => ExceptionCode::Breakpoint,
            Self::LoadAddressMisaligned(_) => ExceptionCode::LoadAddressMisaligned,
            Self::LoadAccessFault(_) => ExceptionCode::LoadAccessFault,
            Self::StoreOrAmoAddressMisaligned(_) => ExceptionCode::StoreOrAmoAddressMisaligned,
            Self::StoreOrAmoAccessFault(_) => ExceptionCode::StoreOrAmoAccessFault,
            Self::EnvironmentCallFromUMode => ExceptionCode::EnvironmentCallFromUMode,
            Self::EnvironmentCallFromSMode => ExceptionCode::EnvironmentCallFromSMode,
            Self::EnvironmentCallFromMMode => ExceptionCode::EnvironmentCallFromMMode,
            Self::InstructionPageFault(_) => ExceptionCode::InstructionPageFault,
            Self::LoadPageFault(_) => ExceptionCode::LoadPageFault,
            Self::StoreOrAmoPageFault(_) => ExceptionCode::StoreOrAmoPageFault,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub enum Interrupt {
    SupervisorSoftwareInterrupt = 1,
    MachineSoftwareInterrupt = 3,
    SupervisorTimerInterrupt = 5,
    MachineTimerInterrupt = 7,
    SupervisorExternalInterrupt = 9,
    MachineExternalInterrupt = 11,
}

impl TryFrom<u32> for Interrupt {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::SupervisorSoftwareInterrupt),
            3 => Ok(Self::MachineSoftwareInterrupt),
            5 => Ok(Self::SupervisorTimerInterrupt),
            7 => Ok(Self::MachineTimerInterrupt),
            9 => Ok(Self::SupervisorExternalInterrupt),
            11 => Ok(Self::MachineExternalInterrupt),
            _ => Err(format!("unsupported interrupt code: {value}")),
        }
    }
}
