//! Provides a simulatable RV32I core implementation.

mod execute;
mod mmu;

use crate::core::mmu::MemoryError;
use crate::cs_registers::CSRegisters;
use crate::instruction::{
    BranchCondition, Instruction, LoadWidth, RegImmOp, RegRegOp, RegShiftImmOp, StoreWidth,
};
use crate::registers::Registers;
use crate::simulator::Simulatable;
use crate::system_bus::SystemBus;
use crate::{Alignment, Allocated, Allocator, Endianness};
use execute::Executor;
use mmu::Mmu;
use std::fmt::Debug;

// NOTE: For now `Default` is derived, but this will probably need to be changed to a custom impl.
#[derive(Debug, Default, Clone)]
pub struct Config {
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
#[derive(Debug)]
pub struct Core<A: Allocator, B: SystemBus<A>> {
    config: Config,
    cs_registers: CSRegisters<A>,
    registers: Allocated<A, Registers>,
    system_bus: B,
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    pub fn new(allocator: &mut A, system_bus: B, config: Config) -> Self {
        let cs_registers = CSRegisters::new(allocator);
        let registers = Allocated::new(allocator, Registers::new());
        Self {
            config,
            cs_registers,
            registers,
            system_bus,
        }
    }

    pub fn drop(self, allocator: &mut A) {
        self.registers.drop(allocator);
    }

    /// Force this core to its resets state.
    pub fn reset(&self, allocator: &mut A) {
        self.cs_registers.reset(allocator);
        *self.registers.get_mut(allocator) = Registers::new();
    }

    /// Provide a read-only view of this core's configuration.
    ///
    /// It is not possible to modify the configuration after creation.
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn cs_registers(&self) -> &CSRegisters<A> {
        &self.cs_registers
    }

    pub fn registers<'a>(&self, allocator: &'a A) -> &'a Registers {
        self.registers.get(allocator)
    }

    pub fn registers_mut<'a>(&self, allocator: &'a mut A) -> &'a mut Registers {
        self.registers.get_mut(allocator)
    }

    /// Returns the endianness of the core in the current privilege mode.
    pub fn endianness(&self) -> Endianness {
        todo!()
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
                return ExecutionResult::Exception(Exception::IllegalInstruction);
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
            ExecutionResult::Ok => {}
            ExecutionResult::Exception(_) => todo!(),
            ExecutionResult::Interrupt(_) => todo!(),
        }

        // TODO: update counters
    }

    fn drop(self, allocator: &mut A) {
        self.drop(allocator);
    }
}

#[derive(Debug, Default)]
pub enum ExecutionResult {
    /// Execution went normal
    #[default]
    Ok,
    /// Execution triggered an exception
    Exception(Exception),
    /// Execution triggered an interrupt
    Interrupt(Interrupt), // TODO
}

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
