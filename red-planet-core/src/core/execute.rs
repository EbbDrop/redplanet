use super::memory::{MemoryError, CORE_ENDIAN};
use crate::bus::Bus;
use crate::core::{ConnectedCore, Exception, ExecutionResult};
use crate::instruction::FenceOrderCombination;
use crate::registers::{Registers, Specifier};
use crate::{Alignment, Allocator};

#[derive(Debug)]
pub(super) struct Executor<'a, 'c, A: Allocator, B: Bus<A>> {
    pub allocator: &'a mut A,
    pub core: &'c ConnectedCore<A, B>,
}

impl<'a, 'c, A: Allocator, B: Bus<A>> Executor<'a, 'c, A, B> {
    /// Executes an `addi` instruction.
    ///
    /// Corresponds to the assembly instruction `addi dest src immediate`.
    ///
    /// > ADDI adds the sign-extended 12-bit immediate to register rs1. Arithmetic overflow is
    /// > ignored and the result is simply the low XLEN bits of the result. ADDI rd, rs1, 0 is used
    /// > to implement the MV rd, rs1 assembler pseudoinstruction.
    pub fn addi(&mut self, dest: Specifier, src: Specifier, immediate: i32) -> ExecutionResult {
        self.reg_imm_op(dest, src, immediate, |s, imm| s.wrapping_add_signed(imm))
    }

    /// Executes a `slti` instruction.
    ///
    /// Corresponds to the assembly instruction `slti dest src immediate`.
    ///
    /// > SLTI (set less than immediate) places the value 1 in register rd if register rs1 is less
    /// > than the sign-extended immediate when both are treated as signed numbers, else 0 is
    /// > written to rd.
    pub fn slti(&mut self, dest: Specifier, src: Specifier, immediate: i32) -> ExecutionResult {
        self.reg_imm_op(dest, src, immediate, |s, imm| ((s as i32) < imm) as u32)
    }

    /// Executes a `sltiu` instruction.
    ///
    /// Corresponds to the assembly instruction `sltiu dest src immediate`.
    ///
    /// > SLTI (set less than immediate) places the value 1 in register rd if register rs1 is less
    /// > than the sign-extended immediate when both are treated as signed numbers, else 0 is
    /// > written to rd. SLTIU is similar but compares the values as unsigned numbers (i.e., the
    /// > immediate is first sign-extended to XLEN bits then treated as an unsigned number). Note,
    /// > SLTIU rd, rs1, 1 sets rd to 1 if rs1 equals zero, otherwise sets rd to 0 (assembler
    /// > pseudoinstruction SEQZ rd, rs).
    pub fn sltiu(&mut self, dest: Specifier, src: Specifier, immediate: i32) -> ExecutionResult {
        self.reg_imm_op(dest, src, immediate, |s, imm| (s < (imm as u32)) as u32)
    }

    /// Executes an `andi` instruction.
    ///
    /// Corresponds to the assembly instruction `andi dest src immediate`.
    ///
    /// > ANDI, ORI, XORI are logical operations that perform bitwise AND, OR, and XOR on register
    /// > rs1 and the sign-extended 12-bit immediate and place the result in rd.
    pub fn andi(&mut self, dest: Specifier, src: Specifier, immediate: i32) -> ExecutionResult {
        self.reg_imm_op(dest, src, immediate, |s, imm| s & (imm as u32))
    }

    /// Executes an `ori` instruction.
    ///
    /// Corresponds to the assembly instruction `ori dest src immediate`.
    ///
    /// > ANDI, ORI, XORI are logical operations that perform bitwise AND, OR, and XOR on register
    /// > rs1 and the sign-extended 12-bit immediate and place the result in rd.
    pub fn ori(&mut self, dest: Specifier, src: Specifier, immediate: i32) -> ExecutionResult {
        self.reg_imm_op(dest, src, immediate, |s, imm| s | (imm as u32))
    }

    /// Executes a `xori` instruction.
    ///
    /// Corresponds to the assembly instruction `xori dest src immediate`.
    ///
    /// > ANDI, ORI, XORI are logical operations that perform bitwise AND, OR, and XOR on register
    /// > rs1 and the sign-extended 12-bit immediate and place the result in rd. Note, XORI rd, rs1,
    /// > -1 performs a bitwise logical inversion of register rs1 (assembler pseudoinstruction NOT
    /// > rd, rs).
    pub fn xori(&mut self, dest: Specifier, src: Specifier, immediate: i32) -> ExecutionResult {
        self.reg_imm_op(dest, src, immediate, |s, imm| s ^ (imm as u32))
    }

    /// Executes a `slli` instruction.
    ///
    /// Corresponds to the assembly instruction `slli dest src shift_amount_u5`.
    ///
    /// > SLLI is a logical left shift (zeros are shifted into the lower bits).
    ///
    /// # Panics
    ///
    /// `shift_amount` must fit in a u5 (`0..=31`), otherwise this will panic.
    pub fn slli(
        &mut self,
        dest: Specifier,
        src: Specifier,
        shift_amount_u5: u32,
    ) -> ExecutionResult {
        self.reg_shamt_op(dest, src, shift_amount_u5, |s, shamt| s << shamt)
    }

    /// Executes a `srli` instruction.
    ///
    /// Corresponds to the assembly instruction `srli dest src shift_amount_u5`.
    ///
    /// > SRLI is a logical right shift (zeros are shifted into the upper bits).
    ///
    /// # Panics
    ///
    /// `shift_amount` must fit in a u5 (`0..=31`), otherwise this will panic.
    pub fn srli(
        &mut self,
        dest: Specifier,
        src: Specifier,
        shift_amount_u5: u32,
    ) -> ExecutionResult {
        self.reg_shamt_op(dest, src, shift_amount_u5, |s, shamt| s >> shamt)
    }

    /// Executes a `srai` instruction.
    ///
    /// Corresponds to the assembly instruction `srai dest src shift_amount_u5`.
    ///
    /// > SRAI is an arithmetic right shift (the original sign bit is copied into the vacated upper
    /// > bits).
    ///
    /// # Panics
    ///
    /// `shift_amount` must fit in a u5 (`0..=31`), otherwise this will panic.
    pub fn srai(
        &mut self,
        dest: Specifier,
        src: Specifier,
        shift_amount_u5: u32,
    ) -> ExecutionResult {
        self.reg_shamt_op(dest, src, shift_amount_u5, |s, shamt| {
            ((s as i32) >> shamt) as u32
        })
    }

    /// Executes a `lui` instruction.
    ///
    /// Corresponds to the assembly instruction `lui dest immediate`.
    ///
    /// > LUI (load upper immediate) is used to build 32-bit constants and uses the U-type format.
    /// > LUI places the U-immediate value in the top 20 bits of the destination register rd,
    /// > filling in the lowest 12 bits with zeros.
    ///
    /// Note that the bottom 12 bits of `immediate` need not be zero, they will always be discarded.
    pub fn lui(&mut self, dest: Specifier, immediate: i32) -> ExecutionResult {
        let result = immediate as u32 & !0xFFF;
        let registers = self.core.registers_mut(self.allocator);
        registers.set_x(dest, result);
        increment_pc(registers);
        ExecutionResult::Ok
    }

    /// Executes an `auipc` instruction.
    ///
    /// Corresponds to the assembly instruction `auipc dest immediate`.
    ///
    /// > AUIPC (add upper immediate to pc) is used to build pc-relative addresses and uses the
    /// > U-type format. AUIPC forms a 32-bit offset from the 20-bit U-immediate, filling in the
    /// > lowest 12 bits with zeros, adds this offset to the address of the AUIPC instruction, then
    /// > places the result in register rd.
    ///
    /// Note that the bottom 12 bits of `immediate` need not be zero, this will take care of that.
    pub fn auipc(&mut self, dest: Specifier, immediate: i32) -> ExecutionResult {
        let registers = self.core.registers_mut(self.allocator);
        let result = registers.pc().wrapping_add_signed(immediate & !0xFFF);
        registers.set_x(dest, result);
        increment_pc(registers);
        ExecutionResult::Ok
    }

    /// Executes an `add` instruction.
    ///
    /// Corresponds to the assembly instruction `add dest src1 src2`.
    ///
    /// > ADD performs the addition of rs1 and rs2.
    pub fn add(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| s1.wrapping_add(s2))
    }

    /// Executes a `sub` instruction.
    ///
    /// Corresponds to the assembly instruction `sub dest src1 src2`.
    ///
    /// > SUB performs the subtraction of rs2 from rs1.
    pub fn sub(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| s1.wrapping_sub(s2))
    }

    /// Executes a `slt` instruction.
    ///
    /// Corresponds to the assembly instruction `slt dest src1 src2`.
    ///
    /// > SLT and SLTU perform signed and unsigned compares respectively, writing 1 to rd if
    /// > rs1 < rs2, 0 otherwise.
    pub fn slt(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| {
            ((s1 as i32) < (s2 as i32)) as u32
        })
    }

    /// Executes a `sltu` instruction.
    ///
    /// Corresponds to the assembly instruction `sltu dest src1 src2`.
    ///
    /// > SLT and SLTU perform signed and unsigned compares respectively, writing 1 to rd if
    /// > rs1 < rs2, 0 otherwise. Note, SLTU rd, x0, rs2 sets rd to 1 if rs2 is not equal to zero,
    /// > otherwise sets rd to zero (assembler pseudoinstruction SNEZ rd, rs).
    pub fn sltu(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| (s1 < s2) as u32)
    }

    /// Executes an `and` instruction.
    ///
    /// Corresponds to the assembly instruction `and dest src1 src2`.
    ///
    /// > AND, OR, and XOR perform bitwise logical operations.
    pub fn and(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| s1 & s2)
    }

    /// Executes an `or` instruction.
    ///
    /// Corresponds to the assembly instruction `or dest src1 src2`.
    ///
    /// > AND, OR, and XOR perform bitwise logical operations.
    pub fn or(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| s1 | s2)
    }

    /// Executes an `xor` instruction.
    ///
    /// Corresponds to the assembly instruction `xor dest src1 src2`.
    ///
    /// > AND, OR, and XOR perform bitwise logical operations.
    pub fn xor(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| s1 ^ s2)
    }

    /// Executes a `sll` instruction.
    ///
    /// Corresponds to the assembly instruction `sll dest src1 src2`.
    ///
    /// > SLL, SRL, and SRA perform logical left, logical right, and arithmetic right shifts on the
    /// > value in register rs1 by the shift amount held in the lower 5 bits of register rs2.
    pub fn sll(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| s1 << (s2 & 0x1F))
    }

    /// Executes a `srl` instruction.
    ///
    /// Corresponds to the assembly instruction `srl dest src1 src2`.
    ///
    /// > SLL, SRL, and SRA perform logical left, logical right, and arithmetic right shifts on the
    /// > value in register rs1 by the shift amount held in the lower 5 bits of register rs2.
    pub fn srl(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| s1 >> (s2 & 0x1F))
    }

    /// Executes a `sra` instruction.
    ///
    /// Corresponds to the assembly instruction `sra dest src1 src2`.
    ///
    /// > SLL, SRL, and SRA perform logical left, logical right, and arithmetic right shifts on the
    /// > value in register rs1 by the shift amount held in the lower 5 bits of register rs2.
    pub fn sra(&mut self, dest: Specifier, src1: Specifier, src2: Specifier) -> ExecutionResult {
        self.reg_reg_op(dest, src1, src2, |s1, s2| {
            ((s1 as i32) >> (s2 & 0x1F)) as u32
        })
    }

    pub fn jal(&mut self, dest: Specifier, offset: i32) -> ExecutionResult {
        self.jump_op(dest, |registers| registers.pc().wrapping_add_signed(offset))
    }

    pub fn jalr(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.jump_op(dest, |registers| {
            registers.x(base).wrapping_add_signed(offset) & !1
        })
    }

    pub fn beq(&mut self, src1: Specifier, src2: Specifier, offset: i32) -> ExecutionResult {
        self.cond_branch(src1, src2, offset, |s1, s2| s1 == s2)
    }

    pub fn bne(&mut self, src1: Specifier, src2: Specifier, offset: i32) -> ExecutionResult {
        self.cond_branch(src1, src2, offset, |s1, s2| s1 != s2)
    }

    pub fn blt(&mut self, src1: Specifier, src2: Specifier, offset: i32) -> ExecutionResult {
        self.cond_branch(src1, src2, offset, |s1, s2| (s1 as i32) < (s2 as i32))
    }

    pub fn bltu(&mut self, src1: Specifier, src2: Specifier, offset: i32) -> ExecutionResult {
        self.cond_branch(src1, src2, offset, |s1, s2| s1 < s2)
    }

    pub fn bge(&mut self, src1: Specifier, src2: Specifier, offset: i32) -> ExecutionResult {
        self.cond_branch(src1, src2, offset, |s1, s2| (s1 as i32) >= (s2 as i32))
    }

    pub fn bgeu(&mut self, src1: Specifier, src2: Specifier, offset: i32) -> ExecutionResult {
        self.cond_branch(src1, src2, offset, |s1, s2| s1 >= s2)
    }

    pub fn lb(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .memory()
                .read_byte(this.allocator, address)
                .map(|value| value as i8 as u32)
        })
    }

    pub fn lbu(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .memory()
                .read_byte(this.allocator, address)
                .map(|value| value as u32)
        })
    }

    pub fn lh(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .memory()
                .read_halfword::<CORE_ENDIAN>(this.allocator, address)
                .map(|value| value as i16 as u32)
        })
    }

    pub fn lhu(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .memory()
                .read_halfword::<CORE_ENDIAN>(this.allocator, address)
                .map(|value| value as u32)
        })
    }

    pub fn lw(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .memory()
                .read_word::<CORE_ENDIAN>(this.allocator, address)
        })
    }

    pub fn sb(&mut self, src: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.store_op(src, base, offset, |this, address, value| {
            this.core
                .memory()
                .write_byte(this.allocator, address, value as u8)
        })
    }

    pub fn sh(&mut self, src: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.store_op(src, base, offset, |this, address, value| {
            this.core
                .memory()
                .write_halfword::<CORE_ENDIAN>(this.allocator, address, value as u16)
        })
    }

    pub fn sw(&mut self, src: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.store_op(src, base, offset, |this, address, value| {
            this.core
                .memory()
                .write_word::<CORE_ENDIAN>(this.allocator, address, value)
        })
    }

    pub fn fence(
        &mut self,
        predecessor: FenceOrderCombination,
        successor: FenceOrderCombination,
    ) -> ExecutionResult {
        // Since only one core is supported, this is equivalent to a nop instruction.
        let _ = predecessor;
        let _ = successor;
        increment_pc(self.core.registers_mut(self.allocator));
        ExecutionResult::Ok
    }

    pub fn ecall(&mut self) -> ExecutionResult {
        todo!()
    }

    pub fn ebreak(&mut self) -> ExecutionResult {
        todo!()
    }

    #[inline]
    fn reg_imm_op<F>(
        &mut self,
        dest: Specifier,
        src: Specifier,
        immediate: i32,
        op: F,
    ) -> ExecutionResult
    where
        F: FnOnce(u32, i32) -> u32,
    {
        let registers = self.core.registers_mut(self.allocator);
        registers.set_x(dest, op(registers.x(src), immediate));
        increment_pc(registers);
        ExecutionResult::Ok
    }

    #[inline]
    fn reg_shamt_op<F>(
        &mut self,
        dest: Specifier,
        src: Specifier,
        shift_amount_u5: u32,
        op: F,
    ) -> ExecutionResult
    where
        F: FnOnce(u32, u32) -> u32,
    {
        if shift_amount_u5 > 31 {
            panic!("out of range u5 used");
        }
        let registers = self.core.registers_mut(self.allocator);
        registers.set_x(dest, op(registers.x(src), shift_amount_u5));
        increment_pc(registers);
        ExecutionResult::Ok
    }

    #[inline]
    fn reg_reg_op<F>(
        &mut self,
        dest: Specifier,
        src1: Specifier,
        src2: Specifier,
        op: F,
    ) -> ExecutionResult
    where
        F: FnOnce(u32, u32) -> u32,
    {
        let registers = self.core.registers_mut(self.allocator);
        registers.set_x(dest, op(registers.x(src1), registers.x(src2)));
        increment_pc(registers);
        ExecutionResult::Ok
    }

    fn jump_op<F>(&mut self, dest: Specifier, compute_target: F) -> ExecutionResult
    where
        F: FnOnce(&Registers) -> u32,
    {
        let registers = self.core.registers_mut(self.allocator);
        // Compute target pc
        let new_pc = compute_target(registers);
        // Check target pc is word-aligned
        if !Alignment::WORD.is_aligned(new_pc) {
            return ExecutionResult::Exception(Exception::InstructionAddressMisaligned);
        }
        // Update pc to target
        let old_pc = std::mem::replace(registers.pc_mut(), new_pc);
        // Write incremented old pc to `dest` register
        registers.set_x(dest, old_pc.wrapping_add(4));
        ExecutionResult::Ok
    }

    // Takes the branch if `predicate` returns `true`.
    fn cond_branch<P>(
        &mut self,
        src1: Specifier,
        src2: Specifier,
        offset: i32,
        predicate: P,
    ) -> ExecutionResult
    where
        P: FnOnce(u32, u32) -> bool,
    {
        let registers = self.core.registers_mut(self.allocator);
        if predicate(registers.x(src1), registers.x(src2)) {
            let new_pc = registers.pc().wrapping_add_signed(offset);
            // Check target pc is word-aligned
            if !Alignment::WORD.is_aligned(new_pc) {
                return ExecutionResult::Exception(Exception::InstructionAddressMisaligned);
            }
            *registers.pc_mut() = new_pc;
        } else {
            increment_pc(registers);
        }
        ExecutionResult::Ok
    }

    #[inline]
    fn load_op<F>(
        &mut self,
        dest: Specifier,
        base: Specifier,
        offset: i32,
        op: F,
    ) -> ExecutionResult
    where
        F: FnOnce(&mut Self, u32) -> Result<u32, MemoryError>,
    {
        let registers = self.core.registers(self.allocator);
        let address = registers.x(base).wrapping_add_signed(offset);
        match op(self, address) {
            Ok(value) => {
                let registers = self.core.registers_mut(self.allocator);
                registers.set_x(dest, value);
                increment_pc(registers);
                ExecutionResult::Ok
            }
            Err(err) => match err {
                MemoryError::MisalignedAccess => {
                    ExecutionResult::Exception(Exception::LoadAddressMisaligned)
                }
                MemoryError::AccessFault => ExecutionResult::Exception(Exception::LoadAccessFault),
                MemoryError::EffectfulReadOnly => unreachable!(),
            },
        }
    }

    #[inline]
    fn store_op<F>(
        &mut self,
        src: Specifier,
        base: Specifier,
        offset: i32,
        op: F,
    ) -> ExecutionResult
    where
        F: FnOnce(&mut Self, u32, u32) -> Result<(), MemoryError>,
    {
        let registers = self.core.registers(self.allocator);
        let value = registers.x(src);
        let address = registers.x(base).wrapping_add_signed(offset);
        match op(self, address, value) {
            Ok(()) => {
                increment_pc(self.core.registers_mut(self.allocator));
                ExecutionResult::Ok
            }
            Err(err) => match err {
                MemoryError::MisalignedAccess => {
                    ExecutionResult::Exception(Exception::StoreOrAmoAddressMisaligned)
                }
                MemoryError::AccessFault => {
                    ExecutionResult::Exception(Exception::StoreOrAmoAccessFault)
                }
                MemoryError::EffectfulReadOnly => unreachable!(),
            },
        }
    }
}

fn increment_pc(registers: &mut Registers) {
    let pc = registers.pc_mut();
    *pc = pc.wrapping_add(4);
}
