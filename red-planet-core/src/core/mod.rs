//! Provides a simulatable RV32I core implementation.

mod counters;
pub mod csr;
mod execute;
mod mmu;
mod status;

use crate::core::mmu::MemoryError;
use crate::instruction::{
    BranchCondition, CsrOp, Instruction, LoadWidth, RegImmOp, RegRegOp, RegShiftImmOp, StoreWidth,
};
use crate::registers::Registers;
use crate::simulator::Simulatable;
use crate::system_bus::SystemBus;
use crate::{Alignment, Allocated, Allocator, Endianness, PrivilegeLevel, RawPrivilegeLevel};
use execute::Executor;
use mmu::Mmu;
use std::fmt::Debug;
use thiserror::Error;

pub use counters::Counters;
pub use csr::CsrSpecifier;
pub use status::Status;

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
    config: Config,
    system_bus: B,
    registers: Allocated<A, Registers>,
    /// Index in the allocator where all CSR counter registers are stored.
    ///
    /// These are allocated together, since at least a subset of them will be updated every tick,
    /// and most likely more will be updated in between snapshots.
    counters: Allocated<A, Counters>,
    status: Allocated<A, Status>,
    privilege_mode: Allocated<A, PrivilegeLevel>,
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

    pub fn new(allocator: &mut A, system_bus: B, config: Config) -> Self {
        let registers = Allocated::new(allocator, Registers::new());
        Self {
            config,
            system_bus,
            registers,
            counters: Allocated::new(allocator, Counters::new()),
            status: Allocated::new(allocator, Status::new()),
            privilege_mode: Allocated::new(allocator, PrivilegeLevel::Machine),
        }
    }

    pub fn drop(self, allocator: &mut A) {
        self.registers.drop(allocator);
    }

    /// Force this core to its resets state.
    pub fn reset(&self, allocator: &mut A) {
        *self.registers.get_mut(allocator) = Registers::new();
        *self.counters.get_mut(allocator) = Counters::new();
    }

    /// Provide a read-only view of this core's configuration.
    ///
    /// It is not possible to modify the configuration after creation.
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn registers<'a>(&self, allocator: &'a A) -> &'a Registers {
        self.registers.get(allocator)
    }

    pub fn registers_mut<'a>(&self, allocator: &'a mut A) -> &'a mut Registers {
        self.registers.get_mut(allocator)
    }

    pub fn status<'a>(&self, allocator: &'a A) -> &'a Status {
        self.status.get(allocator)
    }

    pub fn status_mut<'a>(&self, allocator: &'a mut A) -> &'a mut Status {
        self.status.get_mut(allocator)
    }

    pub fn counters<'a>(&self, allocator: &'a A) -> &'a Counters {
        self.counters.get(allocator)
    }

    pub fn counters_mut<'a>(&self, allocator: &'a mut A) -> &'a mut Counters {
        self.counters.get_mut(allocator)
    }

    /// Returns the current privilege mode the core is in.
    ///
    /// See also [`PrivilegeLevel`].
    pub fn privilege_mode(&self, allocator: &A) -> PrivilegeLevel {
        *self.privilege_mode.get(allocator)
    }

    /// Returns the endianness for the core's current privilege level.
    pub fn endianness(&self, allocator: &A) -> Endianness {
        let status = self.status.get(allocator);
        let be = match self.privilege_mode.get(allocator) {
            PrivilegeLevel::User => status.ube(),
            PrivilegeLevel::Supervisor => status.sbe(),
            PrivilegeLevel::Machine => status.mbe(),
        };
        match be {
            true => Endianness::BE,
            false => Endianness::LE,
        }
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
            csr::MHARTID => Ok(self.config.hart_id),
            //
            // Counter registers
            //
            // cycle
            csr::CYCLE => Ok(self.counters.get(allocator).read_cycle()),
            csr::CYCLEH => Ok(self.counters.get(allocator).read_cycleh()),
            csr::MCYCLE => Ok(self.counters.get(allocator).read_mcycle()),
            csr::MCYCLEH => Ok(self.counters.get(allocator).read_mcycleh()),
            // instret
            csr::INSTRET => Ok(self.counters.get(allocator).read_instret()),
            csr::INSTRETH => Ok(self.counters.get(allocator).read_instreth()),
            csr::MINSTRET => Ok(self.counters.get(allocator).read_minstret()),
            csr::MINSTRETH => Ok(self.counters.get(allocator).read_minstreth()),
            // time
            csr::TIME => Ok(self.read_mtime(allocator) as u32),
            csr::TIMEH => Ok((self.read_mtime(allocator) >> 32) as u32),
            // hpmcounter
            csr::HPMCOUNTER3..=csr::HPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::HPMCOUNTER3);
                Ok(self.counters.get(allocator).read_hpmcounter(offset as u8))
            }
            csr::HPMCOUNTER3H..=csr::HPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::HPMCOUNTER3H);
                Ok(self.counters.get(allocator).read_hpmcounterh(offset as u8))
            }
            csr::MHPMCOUNTER3..=csr::MHPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3);
                Ok(self.counters.get(allocator).read_mhpmcounter(offset as u8))
            }
            csr::MHPMCOUNTER3H..=csr::MHPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3H);
                Ok(self.counters.get(allocator).read_mhpmcounterh(offset as u8))
            }
            csr::MHPMEVENT3..=csr::MHPMEVENT31 => {
                let offset = 3 + (specifier - csr::MHPMEVENT3);
                Ok(self.counters.get(allocator).read_mhpmevent(offset as u8))
            }
            _ => todo!(),
        }
    }

    pub fn write_csr(
        &self,
        allocator: &mut A,
        specifier: CsrSpecifier,
        privilege_level: PrivilegeLevel,
        value: u32,
        mask: u32,
    ) -> Result<(), CsrWriteError> {
        self.check_csr_access(allocator, specifier, privilege_level)
            .map_err(CsrWriteError::AccessError)?;
        if csr::is_read_only(specifier) {
            return Err(CsrWriteError::WriteToReadOnly);
        }
        match specifier {
            //
            // Machine info registers
            //
            // The machine info registers are read-only WARL in this implementation.
            csr::MISA => {}
            csr::MVENDORID => {}
            csr::MARCHID => {}
            csr::MIMPID => {}
            csr::MHARTID => {}
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
            csr::MCYCLE => self.counters.get_mut(allocator).write_mcycle(value, mask),
            csr::MCYCLEH => self.counters.get_mut(allocator).write_mcycleh(value, mask),
            csr::MINSTRET => self.counters.get_mut(allocator).write_minstret(value, mask),
            csr::MINSTRETH => self
                .counters
                .get_mut(allocator)
                .write_minstreth(value, mask),
            csr::MHPMCOUNTER3..=csr::MHPMCOUNTER31 => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3);
                self.counters
                    .get(allocator)
                    .write_mhpmcounter(offset as u8, value, mask);
            }
            csr::MHPMCOUNTER3H..=csr::MHPMCOUNTER31H => {
                let offset = 3 + (specifier - csr::MHPMCOUNTER3H);
                self.counters
                    .get(allocator)
                    .write_mhpmcounterh(offset as u8, value, mask);
            }
            csr::MHPMEVENT3..=csr::MHPMEVENT31 => {
                let offset = 3 + (specifier - csr::MHPMEVENT3);
                self.counters
                    .get(allocator)
                    .write_mhpmevent(offset as u8, value, mask);
            }
            _ => todo!(),
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

    pub fn read_mtime(&self, allocator: &mut A) -> u64 {
        let mut buf = [0u8; 8];
        self.system_bus
            .read(&mut buf, allocator, self.config.mtime_address);
        u64::from_le_bytes(buf)
    }

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

    /// Execute a single instruction on this core.
    ///
    /// This is not the same as [`tick`](Self::tick)! This only takes care of executing the
    /// instruction-specific operations, such as updating `x` registers, updating memory, updating
    /// the `pc` register, and depending on the instruction also updating CSRs. However, additional
    /// state updates that normally happen at a tick, such as incrementing the appropriate counters,
    /// are not performed.
    ///
    /// This can be useful for execution the operation defined by an instruction, without actually
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
        let instruction = match Instruction::decode(raw_instruction) {
            Ok(instruction) => instruction,
            // TODO: match on the error
            Err(_) => {
                return Err(Exception::IllegalInstruction);
            }
        };
        self.execute_instruction(allocator, instruction)
    }

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
        if !Alignment::WORD.is_aligned(address) {
            return Err(Exception::InstructionAddressMisaligned);
        }
        self.mmu()
            .fetch_instruction(allocator, address)
            .map_err(|err| match err {
                MemoryError::MisalignedAccess => Exception::InstructionAddressMisaligned,
                MemoryError::AccessFault => Exception::InstructionAccessFault,
                MemoryError::EffectfulReadOnly => unreachable!(),
            })
    }

    /// Map a virtual byte address to the corresponding physical byte address.
    ///
    /// Helper for [`Mmu`].
    fn translate_address(&self, address: u32) -> u32 {
        // 1-to-1 mapping for now
        address
    }
}

impl<A: Allocator, B: SystemBus<A>> Simulatable<A> for Core<A, B> {
    fn tick(&self, allocator: &mut A) {
        let pc = self.registers(allocator).pc();

        let raw_instruction = match self.fetch_instruction(allocator, pc) {
            Ok(raw_instruction) => raw_instruction,
            Err(_exception) => todo!(),
        };

        let execution_result = self.execute_raw_instruction(allocator, raw_instruction);

        match execution_result {
            Ok(()) => {}
            Err(_) => todo!(),
        }

        let counters = self.counters.get_mut(allocator);
        counters.increment_cycle();
        counters.increment_instret();
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

/// Result of executing a single instruction. [`Ok`] if execution went normal, [`Err`] if an
/// exception occurred.
pub type ExecutionResult = Result<(), Exception>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Exception {
    /// Instruction address is not on a four-byte aligned boundary in memory.
    InstructionAddressMisaligned,
    InstructionAccessFault,
    /// Generic exception used to communicate one of many possible scenarios:
    ///
    /// - (*UNSPECIFIED*) Attempt to decode a reserved instruction.
    /// - Attempt to access a non-existent CSR.
    /// - Attempt to access a CSR without the appropriate privilege level.
    /// - Attempt to write to a read-only CSR.
    IllegalInstruction,
    Breakpoint,
    LoadAddressMisaligned,
    LoadAccessFault,
    StoreOrAmoAddressMisaligned,
    StoreOrAmoAccessFault,
    EnvironmentCallFromUMode,
    EnvironmentCallFromSMode,
    EnvironmentCallFromMMode,
    InstructionPageFault,
    LoadPageFault,
    StoreOrAmoPageFault,
}

impl Exception {
    /// Returns the exception code (cause) for this exception.
    pub fn code(&self) -> u32 {
        match self {
            Self::InstructionAddressMisaligned => 0,
            Self::InstructionAccessFault => 1,
            Self::IllegalInstruction => 2,
            Self::Breakpoint => 3,
            Self::LoadAddressMisaligned => 4,
            Self::LoadAccessFault => 5,
            Self::StoreOrAmoAddressMisaligned => 6,
            Self::StoreOrAmoAccessFault => 7,
            Self::EnvironmentCallFromUMode => 8,
            Self::EnvironmentCallFromSMode => 9,
            Self::EnvironmentCallFromMMode => 11,
            Self::InstructionPageFault => 12,
            Self::LoadPageFault => 13,
            Self::StoreOrAmoPageFault => 15,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
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
