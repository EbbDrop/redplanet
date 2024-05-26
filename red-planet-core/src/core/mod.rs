//! Provides a simulatable RV32I core implementation.

mod control;
mod counters;
pub mod csr;
mod execute;
mod mconfig;
mod mmu;
mod status;
mod trap;

use crate::core::mmu::MemoryError;
use crate::instruction::{
    BranchCondition, CsrOp, Instruction, LoadWidth, RegImmOp, RegRegOp, RegShiftImmOp, StoreWidth,
};
use crate::registers::Registers;
use crate::simulator::Simulatable;
use crate::system_bus::SystemBus;
use crate::{Allocated, Allocator, Endianness, PrivilegeLevel, RawPrivilegeLevel};
use control::Control;
use counters::Counters;
use execute::Executor;
use mconfig::Mconfig;
use mmu::Mmu;
use status::Status;
use std::fmt::Debug;
use thiserror::Error;
use trap::{Trap, TrapCause, VectorMode};

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
    /// Address to which the core's PC register is reset.
    pub reset_vector: u32,
    /// Address of the handler for Non-Maskable Interrupts.
    pub nmi_vector: u32,
}

/// RISC-V core implementing the RV32I ISA.
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
    /// All CSR counter registers.
    ///
    /// Allocated together, since most of them will be updated simultaneously.
    counters: Allocated<A, Counters>,
    trap: Allocated<A, Trap>,
    control: Allocated<A, Control>,
    mconfig: Allocated<A, Mconfig>,
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    /// The misa CSR is set to `0x4014_0100`, indicating that MXL=32 and that extensions I, S, and U
    /// are supported.
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
    pub const MISA: u32 = 0x4014_0100;
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
        let registers = Allocated::new(allocator, Registers::new(config.reset_vector));
        Self {
            config,
            system_bus,
            registers,
            trap: Allocated::new(allocator, Trap::new()),
            counters: Allocated::new(allocator, Counters::new()),
            status: Allocated::new(allocator, Status::new()),
            privilege_mode: Allocated::new(allocator, PrivilegeLevel::Machine),
            control: Allocated::new(allocator, Control::new()),
            mconfig: Allocated::new(allocator, Mconfig::new()),
        }
    }

    pub fn drop(self, allocator: &mut A) {
        self.registers.drop(allocator);
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
        *self.registers.get_mut(allocator) = Registers::new(self.config.reset_vector);
        self.trap.get_mut(allocator).mcause.set_exception(None);
        *self.counters.get_mut(allocator) = Counters::new();
        let status = self.status.get_mut(allocator);
        status.set_mie(false);
        status.set_mprv(false);
        status.set_mbe(false);
        *self.privilege_mode.get_mut(allocator) = PrivilegeLevel::Machine;
        *self.control.get_mut(allocator) = Control::new();
        *self.mconfig.get_mut(allocator) = Mconfig::new();
    }

    /// Generate a Non-Maskable Interrupt.
    pub fn nmi(&self, allocator: &mut A) {
        let pc = self.registers.get_mut(allocator).pc_mut();
        let old_pc = std::mem::replace(pc, self.config.nmi_vector);
        let trap = self.trap.get_mut(allocator);
        trap.write_mepc(old_pc, 0xFFFF_FFFF);
        trap.mcause.set_interrupt(None);
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
    ) -> Result<u32, CsrAccessError> {
        self.check_csr_access(allocator, specifier, privilege_level)?;
        match specifier {
            //
            // Machine info registers
            //
            csr::MISA => Ok(Self::MISA),
            csr::MVENDORID => Ok(Self::MVENDORID),
            csr::MARCHID => Ok(Self::MARCHID),
            csr::MIMPID => Ok(Self::MIMPID),
            csr::MCONFIGPTR => Ok(Self::MCONFIGPTR),
            csr::MHARTID => Ok(self.config.hart_id),
            //
            // Status registers
            //
            csr::MSTATUS => Ok(self.read_mstatus(allocator)),
            csr::MSTATUSH => Ok(self.read_mstatush(allocator)),
            csr::SSTATUS => Ok(self.read_sstatus(allocator)),
            //
            // Machine trap handling
            //
            csr::MSCRATCH => Ok(self.trap.get(allocator).read_mscratch()),
            csr::MEPC => Ok(self.trap.get(allocator).read_mepc()),
            csr::MCAUSE => Ok(self.trap.get(allocator).mcause.read()),
            csr::MTVAL => Ok(self.trap.get(allocator).read_mtval()),
            csr::MIP => todo!("must be able to write to SEIP"),
            csr::MTINST => Ok(self.trap.get(allocator).read_mtinst()),
            csr::MTVAL2 => Ok(self.trap.get(allocator).read_mtval2()),
            //
            // supervisor trap handling
            //
            csr::SSCRATCH => Ok(self.trap.get(allocator).read_sscratch()),
            csr::SEPC => Ok(self.trap.get(allocator).read_sepc()),
            csr::SCAUSE => Ok(self.trap.get(allocator).scause.read()),
            csr::STVAL => Ok(self.trap.get(allocator).read_stval()),
            csr::SIP => todo!(),
            //
            // Counter registers
            //
            // cycle
            csr::CYCLE => Ok(self.read_cycle(allocator)),
            csr::CYCLEH => Ok(self.read_cycleh(allocator)),
            csr::MCYCLE => Ok(self.read_mcycle(allocator)),
            csr::MCYCLEH => Ok(self.read_mcycleh(allocator)),
            // instret
            csr::INSTRET => Ok(self.read_instret(allocator)),
            csr::INSTRETH => Ok(self.read_instreth(allocator)),
            csr::MINSTRET => Ok(self.read_minstret(allocator)),
            csr::MINSTRETH => Ok(self.read_minstreth(allocator)),
            // time
            csr::TIME => Ok(self.read_mtime(allocator) as u32),
            csr::TIMEH => Ok((self.read_mtime(allocator) >> 32) as u32),
            // hpmcounter
            csr::HPMCOUNTER3..=csr::HPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::HPMCOUNTER3);
                Ok(self.read_hpmcounter(allocator, offset as u8))
            }
            csr::HPMCOUNTER3H..=csr::HPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::HPMCOUNTER3H);
                Ok(self.read_hpmcounterh(allocator, offset as u8))
            }
            csr::MHPMCOUNTER3..=csr::MHPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3);
                Ok(self.read_mhpmcounter(allocator, offset as u8))
            }
            csr::MHPMCOUNTER3H..=csr::MHPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3H);
                Ok(self.read_mhpmcounterh(allocator, offset as u8))
            }
            //
            // Machine counter setup
            //
            csr::MHPMEVENT3..=csr::MHPMEVENT31 => {
                let offset = 3 + (specifier - csr::MHPMEVENT3);
                Ok(self.read_mhpmevent(allocator, offset as u8))
            }
            csr::MCOUNTINHIBIT => Ok(self.control.get(allocator).mcountinhibit.read()),
            //
            // Trap setup registers
            //
            csr::MTVEC => Ok(self.trap.get(allocator).mtvec.read()),
            csr::MEDELEG => Ok(self.trap.get(allocator).medeleg.read()),
            csr::MCOUNTEREN => Ok(self.control.get(allocator).mcounteren.read()),
            csr::STVEC => Ok(self.trap.get(allocator).stvec.read()),
            csr::SCOUNTEREN => Ok(self.control.get(allocator).scounteren.read()),
            //
            // Machine configuration registers
            //
            csr::MENVCFG => Ok(self.mconfig.get(allocator).read_menvcfg()),
            csr::MENVCFGH => Ok(self.mconfig.get(allocator).read_menvcfgh()),
            csr::MSECCFG | csr::MSECCFGH => Err(CsrAccessError::CsrUnsupported(specifier)),
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
    ) -> Result<(), CsrWriteError> {
        self.check_csr_access(allocator, specifier, privilege_level)?;
        if csr::is_read_only(specifier) {
            return Err(CsrWriteError::WriteToReadOnly);
        }
        match specifier {
            //
            // Machine info registers
            //
            // The machine info registers are read-only or read-only WARL in this implementation.
            csr::MISA => {}
            csr::MVENDORID => {}
            csr::MARCHID => {}
            csr::MIMPID => {}
            csr::MCONFIGPTR => {}
            csr::MHARTID => {}
            //
            // Status registers
            //
            csr::MSTATUS => self.write_mstatus(allocator, value, mask),
            csr::MSTATUSH => self.write_mstatush(allocator, value, mask),
            csr::SSTATUS => self.write_sstatus(allocator, value, mask),
            //
            // Machine trap handling
            //
            csr::MSCRATCH => self.trap.get_mut(allocator).write_mscratch(value, mask),
            csr::MEPC => self.trap.get_mut(allocator).write_mepc(value, mask),
            csr::MCAUSE => self.trap.get_mut(allocator).mcause.write(value, mask),
            csr::MTVAL => self.trap.get_mut(allocator).write_mtval(value, mask),
            csr::MIP => todo!("must be able to write to SEIP"),
            csr::MTINST => self.trap.get_mut(allocator).write_mtinst(value, mask),
            csr::MTVAL2 => self.trap.get_mut(allocator).write_mtval2(value, mask),
            //
            // supervisor trap handling
            //
            csr::SSCRATCH => self.trap.get_mut(allocator).write_sscratch(value, mask),
            csr::SEPC => self.trap.get_mut(allocator).write_sepc(value, mask),
            csr::SCAUSE => self.trap.get_mut(allocator).scause.write(value, mask),
            csr::STVAL => self.trap.get_mut(allocator).write_stval(value, mask),
            csr::SIP => todo!(),
            //
            // Counter registers
            //
            // Non-m-counters are read-only shadows of their m-counter counterparts.
            csr::CYCLE
            | csr::CYCLEH
            | csr::INSTRET
            | csr::INSTRETH
            | csr::TIME
            | csr::TIMEH
            | csr::HPMCOUNTER3..=csr::HPMCOUNTER31
            | csr::HPMCOUNTER3H..=csr::HPMCOUNTER31H => {}
            csr::MCYCLE => self.write_mcycle(allocator, value, mask),
            csr::MCYCLEH => self.write_mcycleh(allocator, value, mask),
            csr::MINSTRET => self.write_minstret(allocator, value, mask),
            csr::MINSTRETH => self.write_minstreth(allocator, value, mask),
            csr::MHPMCOUNTER3..=csr::MHPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3);
                self.write_mhpmcounter(allocator, offset as u8, value, mask);
            }
            csr::MHPMCOUNTER3H..=csr::MHPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3H);
                self.write_mhpmcounterh(allocator, offset as u8, value, mask);
            }
            //
            // Machine counter setup
            //
            csr::MHPMEVENT3..=csr::MHPMEVENT31 => {
                let offset = 3 + (specifier - csr::MHPMEVENT3);
                self.write_mhpmevent(allocator, offset as u8, value, mask);
            }
            csr::MCOUNTINHIBIT => self
                .control
                .get_mut(allocator)
                .mcountinhibit
                .write(value, mask),
            //
            // Trap setup registers
            //
            csr::MTVEC => self.trap.get_mut(allocator).mtvec.write(value, mask),
            csr::MEDELEG => self.trap.get_mut(allocator).medeleg.write(value, mask),
            csr::MCOUNTEREN => self
                .control
                .get_mut(allocator)
                .mcounteren
                .write(value, mask),
            csr::STVEC => self.trap.get_mut(allocator).stvec.write(value, mask),
            csr::SCOUNTEREN => self
                .control
                .get_mut(allocator)
                .scounteren
                .write(value, mask),
            //
            // Machine configuration registers
            //
            csr::MENVCFG => self.mconfig.get_mut(allocator).write_menvcfg(value, mask),
            csr::MENVCFGH => self.mconfig.get_mut(allocator).write_menvcfgh(value, mask),
            csr::MSECCFG | csr::MSECCFGH => Err(CsrAccessError::CsrUnsupported(specifier))?,
            _ => Err(CsrAccessError::CsrUnsupported(specifier))?,
        }
        Ok(())
    }

    fn check_csr_access(
        &self,
        _allocator: &A,
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
    ) -> Result<(), CsrAccessError> {
        if !csr::is_valid(specifier) {
            return Err(CsrAccessError::CsrUnsupported(specifier));
        }
        let required_level = csr::required_privilege_level(specifier);
        if privilege_level < required_level {
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
    pub fn step(&self, allocator: &mut A) {
        let pc = self.registers(allocator).pc();
        let raw_instruction = self.fetch_instruction(allocator, pc);
        self.step_with_raw(allocator, raw_instruction);
    }

    /// Execute a single (raw) instruction.
    pub fn step_with_raw(&self, allocator: &mut A, raw_instruction: ExecutionResult<u32>) {
        let instruction = raw_instruction.and_then(|raw| {
            Instruction::decode(raw).map_err(|_| Exception::IllegalInstruction(Some(raw)))
        });
        self.step_with(allocator, instruction);
    }

    /// Execute a single (decoded) instruction.
    pub fn step_with(&self, allocator: &mut A, instruction: ExecutionResult<Instruction>) {
        let exception = instruction
            .and_then(|instruction| self.execute_instruction(allocator, instruction))
            .err();

        let counters = self.counters.get_mut(allocator);
        counters.increment_cycle();
        match instruction {
            // ECALL and EBREAK are not considered to retire.
            // Similarly, if the instruction fetch failed, then instret should not be incremented.
            Ok(Instruction::Ecall | Instruction::Ebreak) | Err(_) => {}
            _ => counters.increment_instret(),
        };

        if let Some(exception) = exception {
            self.trap(allocator, exception.into());
        }
    }

    /// Execute a single (raw) instruction.
    ///
    /// This is not the same as [`Self::step_with_raw`]! This only takes care of executing the
    /// instruction-specific operations, such as updating `x` registers, updating memory, updating
    /// the `pc` register, and depending on the instruction also updating CSRs. However, additional
    /// state updates that normally happen at a tick, such as incrementing the appropriate counters,
    /// are not performed.
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
    /// Note that this is not the same as [`Self::step_with`]!
    /// See [`Self::execute_raw_instruction`] for why.
    pub fn execute_instruction(
        &self,
        allocator: &mut A,
        instruction: Instruction,
    ) -> ExecutionResult {
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

    /// "Independent instruction fetch unit"
    ///
    /// > The base RISC-V ISA has fixed-length 32-bit instructions that must be naturally aligned on
    /// > 32-bit boundaries.
    ///
    /// > Instructions are stored in memory as a sequence of 16-bit little-endian parcels,
    /// > regardless of memory system endianness. Parcels forming one instruction are stored at
    /// > increasing halfword addresses, with the lowest-addressed parcel holding the
    /// > lowest-numbered bits in the instruction specification.
    fn fetch_instruction(&self, allocator: &mut A, address: u32) -> Result<u32, Exception> {
        self.mmu()
            .fetch_instruction(allocator, address)
            .map_err(|err| match err {
                MemoryError::MisalignedAccess => Exception::InstructionAddressMisaligned(address),
                MemoryError::AccessFault => Exception::InstructionAccessFault(address),
            })
    }

    /// Map a virtual byte address to the corresponding physical byte address.
    ///
    /// Helper for [`Mmu`].
    fn translate_address(&self, address: u32) -> u32 {
        // 1-to-1 mapping for now
        address
    }

    fn trap(&self, allocator: &mut A, cause: TrapCause) {
        let pc = self.registers(allocator).pc();
        let privilege_mode = *self.privilege_mode.get(allocator);
        let trap = self.trap.get_mut(allocator);
        // Determine if we should be delegating. Note that `delegate == true` does not necessarily
        // mean the trap will be handled in S-mode, since traps that occur while running in M-mode
        // are always handled in M-mode. That check is performed later; see `trap_to_s_mode`.
        let delegate = match cause {
            TrapCause::Exception(exception) => trap.medeleg.should_delegate(exception),
            TrapCause::Interrupt(interrupt) => trap.mideleg.should_delegate(interrupt),
        };
        // Determine whether we are trapping into S-mode or M-mode.
        let trap_to_s_mode = match (privilege_mode, delegate) {
            (PrivilegeLevel::Machine, _) | (_, false) => false,
            (_, true) => true,
        };
        // Set xcause register.
        match trap_to_s_mode {
            true => trap.scause.set(&cause),
            false => trap.mcause.set(&cause),
        };
        // Set xepc register.
        match trap_to_s_mode {
            true => trap.write_sepc(pc, 0xFFFF_FFFF),
            false => trap.write_mepc(pc, 0xFFFF_FFFF),
        };
        // Write xtval and mtval2 register.
        let tval = match cause {
            TrapCause::Exception(exception) => match exception {
                Exception::IllegalInstruction(raw_instruction) => raw_instruction.unwrap_or(0),
                Exception::Breakpoint => pc,
                Exception::InstructionAddressMisaligned(vaddr)
                | Exception::InstructionAccessFault(vaddr)
                | Exception::LoadAddressMisaligned(vaddr)
                | Exception::StoreOrAmoAddressMisaligned(vaddr)
                | Exception::LoadAccessFault(vaddr)
                | Exception::StoreOrAmoAccessFault(vaddr)
                | Exception::InstructionPageFault(vaddr)
                | Exception::LoadPageFault(vaddr)
                | Exception::StoreOrAmoPageFault(vaddr) => vaddr,
                Exception::EnvironmentCallFromUMode
                | Exception::EnvironmentCallFromSMode
                | Exception::EnvironmentCallFromMMode => 0,
            },
            TrapCause::Interrupt(_) => 0,
        };
        match trap_to_s_mode {
            true => trap.write_stval(tval, 0xFFFF_FFFF),
            false => {
                trap.write_mtval(tval, 0xFFFF_FFFF);
                trap.write_mtval2(0, 0xFFFF_FFFF);
            }
        };
        // Determine trap handler address base on xtvec register and cause type.
        let tvec = match trap_to_s_mode {
            true => &trap.stvec,
            false => &trap.mtvec,
        };
        let trap_handler_address = match (tvec.mode(), &cause) {
            (VectorMode::Vectored, TrapCause::Interrupt(interrupt)) => {
                tvec.base() + 4 * interrupt.code()
            }
            (VectorMode::Vectored, TrapCause::Exception(_)) | (VectorMode::Direct, _) => {
                tvec.base()
            }
        };
        // Set pc to the correct trap handler.
        *self.registers_mut(allocator).pc_mut() = trap_handler_address;
        // Update fields of status register.
        let status = self.status.get_mut(allocator);
        match trap_to_s_mode {
            true => {
                status.set_spie(status.sie());
                status.set_sie(false);
                status.set_spp(privilege_mode.into());
            }
            false => {
                status.set_mpie(status.mie());
                status.set_mie(false);
                status.set_mpp(privilege_mode.into());
            }
        }
        // Update the core's privilege mode.
        *self.privilege_mode.get_mut(allocator) = match trap_to_s_mode {
            true => PrivilegeLevel::Supervisor,
            false => PrivilegeLevel::Machine,
        };
    }
}

impl<A: Allocator, B: SystemBus<A>> Simulatable<A> for Core<A, B> {
    fn tick(&self, allocator: &mut A) {
        self.step(allocator)
    }

    fn drop(self, allocator: &mut A) {
        self.drop(allocator);
    }
}

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
    pub const INSTRUCTION_ADDRESS_MISALIGNED: u32 = 0;
    pub const INSTRUCTION_ACCESS_FAULT: u32 = 1;
    pub const ILLEGAL_INSTRUCTION: u32 = 2;
    pub const BREAKPOINT: u32 = 3;
    pub const LOAD_ADDRESS_MISALIGNED: u32 = 4;
    pub const LOAD_ACCESS_FAULT: u32 = 5;
    pub const STORE_OR_AMO_ADDRESS_MISALIGNED: u32 = 6;
    pub const STORE_OR_AMO_ACCESS_FAULT: u32 = 7;
    pub const ENVIRONMENT_CALL_FROM_U_MODE: u32 = 8;
    pub const ENVIRONMENT_CALL_FROM_S_MODE: u32 = 9;
    pub const ENVIRONMENT_CALL_FROM_M_MODE: u32 = 11;
    pub const INSTRUCTION_PAGE_FAULT: u32 = 12;
    pub const LOAD_PAGE_FAULT: u32 = 13;
    pub const STORE_OR_AMO_PAGE_FAULT: u32 = 15;

    /// Returns the exception code (cause) for this exception.
    pub const fn code(&self) -> u32 {
        match self {
            Self::InstructionAddressMisaligned(_) => Self::INSTRUCTION_ADDRESS_MISALIGNED,
            Self::InstructionAccessFault(_) => Self::INSTRUCTION_ACCESS_FAULT,
            Self::IllegalInstruction(_) => Self::ILLEGAL_INSTRUCTION,
            Self::Breakpoint => Self::BREAKPOINT,
            Self::LoadAddressMisaligned(_) => Self::LOAD_ADDRESS_MISALIGNED,
            Self::LoadAccessFault(_) => Self::LOAD_ACCESS_FAULT,
            Self::StoreOrAmoAddressMisaligned(_) => Self::STORE_OR_AMO_ADDRESS_MISALIGNED,
            Self::StoreOrAmoAccessFault(_) => Self::STORE_OR_AMO_ACCESS_FAULT,
            Self::EnvironmentCallFromUMode => Self::ENVIRONMENT_CALL_FROM_U_MODE,
            Self::EnvironmentCallFromSMode => Self::ENVIRONMENT_CALL_FROM_S_MODE,
            Self::EnvironmentCallFromMMode => Self::ENVIRONMENT_CALL_FROM_M_MODE,
            Self::InstructionPageFault(_) => Self::INSTRUCTION_PAGE_FAULT,
            Self::LoadPageFault(_) => Self::LOAD_PAGE_FAULT,
            Self::StoreOrAmoPageFault(_) => Self::STORE_OR_AMO_PAGE_FAULT,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Interrupt {
    SupervisorSoftwareInterrupt,
    MachineSoftwareInterrupt,
    SupervisorTimerInterrupt,
    MachineTimerInterrupt,
    SupervisorExternalInterrupt,
    MachineExternalInterrupt,
}

impl Interrupt {
    /// Returns the exception code (cause) for this interrupt.
    pub fn code(&self) -> u32 {
        match self {
            Self::SupervisorSoftwareInterrupt => 1,
            Self::MachineSoftwareInterrupt => 3,
            Self::SupervisorTimerInterrupt => 5,
            Self::MachineTimerInterrupt => 7,
            Self::SupervisorExternalInterrupt => 9,
            Self::MachineExternalInterrupt => 11,
        }
    }
}
