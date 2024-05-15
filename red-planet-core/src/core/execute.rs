use super::mmu::{MemoryError, CORE_ENDIAN};
use crate::core::{Core, CsrSpecifier, Exception, ExecutionResult};
use crate::instruction::{CsrOp, FenceOrderCombination};
use crate::registers::{Registers, Specifier};
use crate::system_bus::SystemBus;
use crate::{Alignment, Allocator};

#[derive(Debug)]
pub(super) struct Executor<'a, 'c, A: Allocator, B: SystemBus<A>> {
    pub allocator: &'a mut A,
    pub core: &'c Core<A, B>,
}

impl<'a, 'c, A: Allocator, B: SystemBus<A>> Executor<'a, 'c, A, B> {
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
        Ok(())
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
        Ok(())
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
                .mmu()
                .read_byte(this.allocator, address)
                .map(|value| value as i8 as u32)
        })
    }

    pub fn lbu(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .mmu()
                .read_byte(this.allocator, address)
                .map(|value| value as u32)
        })
    }

    pub fn lh(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .mmu()
                .read_halfword::<CORE_ENDIAN>(this.allocator, address)
                .map(|value| value as i16 as u32)
        })
    }

    pub fn lhu(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .mmu()
                .read_halfword::<CORE_ENDIAN>(this.allocator, address)
                .map(|value| value as u32)
        })
    }

    pub fn lw(&mut self, dest: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.load_op(dest, base, offset, |this, address| {
            this.core
                .mmu()
                .read_word::<CORE_ENDIAN>(this.allocator, address)
        })
    }

    pub fn sb(&mut self, src: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.store_op(src, base, offset, |this, address, value| {
            this.core
                .mmu()
                .write_byte(this.allocator, address, value as u8)
        })
    }

    pub fn sh(&mut self, src: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.store_op(src, base, offset, |this, address, value| {
            this.core
                .mmu()
                .write_halfword::<CORE_ENDIAN>(this.allocator, address, value as u16)
        })
    }

    pub fn sw(&mut self, src: Specifier, base: Specifier, offset: i32) -> ExecutionResult {
        self.store_op(src, base, offset, |this, address, value| {
            this.core
                .mmu()
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
        Ok(())
    }

    pub fn ecall(&mut self) -> ExecutionResult {
        todo!()
    }

    pub fn ebreak(&mut self) -> ExecutionResult {
        todo!()
    }

    /// Executes a `csrrw` instruction.
    ///
    /// Corresponds to the assembly instruction `csrrw dest csr src`.
    ///
    /// > The CSRRW (Atomic Read/Write CSR) instruction atomically swaps values in the CSRs and
    /// > integer registers. CSRRW reads the old value of the CSR, zero-extends the value to XLEN
    /// > bits, then writes it to integer register rd. The initial value in rs1 is written to the
    /// > CSR. If rd=x0, then the instruction shall not read the CSR and shall not cause any of the
    /// > side effects that might occur on a CSR read.
    ///
    /// > A CSRRW with rs1=x0 will attempt to write zero to the destination CSR.
    ///
    /// > Attempts to access a non-existent CSR raise an illegal instruction exception. Attempts to
    /// > access a CSR without appropriate privilege level or to write a read-only register also
    /// > raise illegal instruction exceptions. A read/write register might also contain some bits
    /// > that are read-only, in which case writes to the read-only bits are ignored.
    pub fn csrrw(&mut self, dest: Specifier, csr: CsrSpecifier, src: Specifier) -> ExecutionResult {
        self.csr_reg_op(CsrOp::ReadWrite, dest, csr, src)
    }

    /// Executes a `csrrs` instruction.
    ///
    /// Corresponds to the assembly instruction `csrrs dest csr src`.
    ///
    /// > The CSRRS (Atomic Read and Set Bits in CSR) instruction reads the value of the CSR,
    /// > zero-extends the value to XLEN bits, and writes it to integer register rd. The initial
    /// > value in integer register rs1 is treated as a bit mask that specifies bit positions to be
    /// > set in the CSR. Any bit that is high in rs1 will cause the corresponding bit to be set in
    /// > the CSR, if that CSR bit is writable. Other bits in the CSR are unaffected (though CSRs
    /// > might have side effects when written).
    ///
    /// > For both CSRRS and CSRRC, if rs1=x0, then the instruction will not write to the CSR at
    /// > all, and so shall not cause any of the side effects that might otherwise occur on a CSR
    /// > write, such as raising illegal instruction exceptions on accesses to read-only CSRs. Both
    /// > CSRRS and CSRRC always read the addressed CSR and cause any read side effects regardless
    /// > of rs1 and rd fields. Note that if rs1 specifies a register holding a zero value other
    /// > than x0, the instruction will still attempt to write the unmodified value back to the CSR
    /// > and will cause any attendant side effects.
    ///
    /// > Attempts to access a non-existent CSR raise an illegal instruction exception. Attempts to
    /// > access a CSR without appropriate privilege level or to write a read-only register also
    /// > raise illegal instruction exceptions. A read/write register might also contain some bits
    /// > that are read-only, in which case writes to the read-only bits are ignored.
    pub fn csrrs(&mut self, dest: Specifier, csr: CsrSpecifier, src: Specifier) -> ExecutionResult {
        self.csr_reg_op(CsrOp::ReadSet, dest, csr, src)
    }

    /// Executes a `csrrc` instruction.
    ///
    /// Corresponds to the assembly instruction `csrrc dest csr src`.
    ///
    /// > The CSRRC (Atomic Read and Clear Bits in CSR) instruction reads the value of the CSR,
    /// > zero-extends the value to XLEN bits, and writes it to integer register rd. The initial
    /// > value in integer register rs1 is treated as a bit mask that specifies bit positions to be
    /// > cleared in the CSR. Any bit that is high in rs1 will cause the corresponding bit to be
    /// > cleared in the CSR, if that CSR bit is writable. Other bits in the CSR are unaffected.
    ///
    /// > For both CSRRS and CSRRC, if rs1=x0, then the instruction will not write to the CSR at
    /// > all, and so shall not cause any of the side effects that might otherwise occur on a CSR
    /// > write, such as raising illegal instruction exceptions on accesses to read-only CSRs. Both
    /// > CSRRS and CSRRC always read the addressed CSR and cause any read side effects regardless
    /// > of rs1 and rd fields. Note that if rs1 specifies a register holding a zero value other
    /// > than x0, the instruction will still attempt to write the unmodified value back to the CSR
    /// > and will cause any attendant side effects. A CSRRW with rs1=x0 will attempt to write zero
    /// > to the destination CSR.
    ///
    /// > Attempts to access a non-existent CSR raise an illegal instruction exception. Attempts to
    /// > access a CSR without appropriate privilege level or to write a read-only register also
    /// > raise illegal instruction exceptions. A read/write register might also contain some bits
    /// > that are read-only, in which case writes to the read-only bits are ignored.
    pub fn csrrc(&mut self, dest: Specifier, csr: CsrSpecifier, src: Specifier) -> ExecutionResult {
        self.csr_reg_op(CsrOp::ReadClear, dest, csr, src)
    }

    /// Executes a `csrrwi` instruction.
    ///
    /// Corresponds to the assembly instruction `csrrwi dest csr immediate`.
    ///
    /// > The CSRRWI, CSRRSI, and CSRRCI variants are similar to CSRRW, CSRRS, and CSRRC
    /// > respectively, except they update the CSR using an XLEN-bit value obtained by
    /// > zero-extending a 5-bit unsigned immediate (uimm[4:0]) field encoded in the rs1 field
    /// > instead of a value from an integer register.
    ///
    /// > For CSRRWI, if rd=x0, then the instruction shall not read the CSR and shall not cause any
    /// > of the side effects that might occur on a CSR read.
    ///
    /// > Attempts to access a non-existent CSR raise an illegal instruction exception. Attempts to
    /// > access a CSR without appropriate privilege level or to write a read-only register also
    /// > raise illegal instruction exceptions. A read/write register might also contain some bits
    /// > that are read-only, in which case writes to the read-only bits are ignored.
    pub fn csrrwi(
        &mut self,
        dest: Specifier,
        csr: CsrSpecifier,
        immediate: u32,
    ) -> ExecutionResult {
        self.csr_imm_op(CsrOp::ReadWrite, dest, csr, immediate)
    }

    /// Executes a `csrrsi` instruction.
    ///
    /// Corresponds to the assembly instruction `csrrsi dest csr immediate`.
    ///
    /// > The CSRRWI, CSRRSI, and CSRRCI variants are similar to CSRRW, CSRRS, and CSRRC
    /// > respectively, except they update the CSR using an XLEN-bit value obtained by
    /// > zero-extending a 5-bit unsigned immediate (uimm[4:0]) field encoded in the rs1 field
    /// > instead of a value from an integer register. For CSRRSI and CSRRCI, if the uimm[4:0] field
    /// > is zero, then these instructions will not write to the CSR, and shall not cause any of the
    /// > side effects that might otherwise occur on a CSR write.
    ///
    /// > Both CSRRSI and CSRRCI will always read the CSR and cause any read side effects regardless
    /// > of rd and rs1 fields.
    ///
    /// > Attempts to access a non-existent CSR raise an illegal instruction exception. Attempts to
    /// > access a CSR without appropriate privilege level or to write a read-only register also
    /// > raise illegal instruction exceptions. A read/write register might also contain some bits
    /// > that are read-only, in which case writes to the read-only bits are ignored.
    pub fn csrrsi(
        &mut self,
        dest: Specifier,
        csr: CsrSpecifier,
        immediate: u32,
    ) -> ExecutionResult {
        self.csr_imm_op(CsrOp::ReadSet, dest, csr, immediate)
    }

    /// Executes a `csrrci` instruction.
    ///
    /// Corresponds to the assembly instruction `csrrci dest csr immediate`.
    ///
    /// > The CSRRWI, CSRRSI, and CSRRCI variants are similar to CSRRW, CSRRS, and CSRRC
    /// > respectively, except they update the CSR using an XLEN-bit value obtained by
    /// > zero-extending a 5-bit unsigned immediate (uimm[4:0]) field encoded in the rs1 field
    /// > instead of a value from an integer register. For CSRRSI and CSRRCI, if the uimm[4:0] field
    /// > is zero, then these instructions will not write to the CSR, and shall not cause any of the
    /// > side effects that might otherwise occur on a CSR write.
    ///
    /// > Both CSRRSI and CSRRCI will always read the CSR and cause any read side effects regardless
    /// > of rd and rs1 fields.
    ///
    /// > Attempts to access a non-existent CSR raise an illegal instruction exception. Attempts to
    /// > access a CSR without appropriate privilege level or to write a read-only register also
    /// > raise illegal instruction exceptions. A read/write register might also contain some bits
    /// > that are read-only, in which case writes to the read-only bits are ignored.
    pub fn csrrci(
        &mut self,
        dest: Specifier,
        csr: CsrSpecifier,
        immediate: u32,
    ) -> ExecutionResult {
        self.csr_imm_op(CsrOp::ReadClear, dest, csr, immediate)
    }

    // Private generic implementations

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
        Ok(())
    }

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
        Ok(())
    }

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
        Ok(())
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
            return Err(Exception::InstructionAddressMisaligned);
        }
        // Update pc to target
        let old_pc = std::mem::replace(registers.pc_mut(), new_pc);
        // Write incremented old pc to `dest` register
        registers.set_x(dest, old_pc.wrapping_add(4));
        Ok(())
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
                return Err(Exception::InstructionAddressMisaligned);
            }
            *registers.pc_mut() = new_pc;
        } else {
            increment_pc(registers);
        }
        Ok(())
    }

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
                Ok(())
            }
            Err(err) => match err {
                MemoryError::MisalignedAccess => Err(Exception::LoadAddressMisaligned),
                MemoryError::AccessFault => Err(Exception::LoadAccessFault),
                MemoryError::EffectfulReadOnly => unreachable!(),
            },
        }
    }

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
                Ok(())
            }
            Err(err) => match err {
                MemoryError::MisalignedAccess => Err(Exception::StoreOrAmoAddressMisaligned),
                MemoryError::AccessFault => Err(Exception::StoreOrAmoAccessFault),
                MemoryError::EffectfulReadOnly => unreachable!(),
            },
        }
    }

    fn csr_reg_op(
        &mut self,
        op: CsrOp,
        dest: Specifier,
        csr: CsrSpecifier,
        src: Specifier,
    ) -> ExecutionResult {
        self.csr_op(
            op,
            dest,
            csr,
            (op == CsrOp::ReadWrite || src != Specifier::X0)
                .then(|| self.core.registers(self.allocator).x(src)),
        )
    }

    fn csr_imm_op(
        &mut self,
        op: CsrOp,
        dest: Specifier,
        csr: CsrSpecifier,
        immediate: u32,
    ) -> ExecutionResult {
        self.csr_op(
            op,
            dest,
            csr,
            (op == CsrOp::ReadWrite || immediate != 0).then_some(immediate),
        )
    }

    fn csr_op(
        &mut self,
        op: CsrOp,
        dest: Specifier,
        csr: CsrSpecifier,
        src_value: Option<u32>,
    ) -> ExecutionResult {
        // Read and store the core's current privilege level, since the CSR read may cause the
        // privilege level to be changed as a side-effect. This CSR operation should be atomic, so
        // both the read and write should be performed at the same, original privilege level.
        let privilege_level = self.core.privilege_level(self.allocator);
        if op != CsrOp::ReadWrite || dest != Specifier::X0 {
            let old_value = self
                .core
                .read_csr(self.allocator, csr, privilege_level)
                .map_err(|_| Exception::IllegalInstruction)?;
            let registers = self.core.registers_mut(self.allocator);
            registers.set_x(dest, old_value);
        };
        if let Some(src_value) = src_value {
            let (value, mask) = match op {
                CsrOp::ReadWrite => (src_value, 0xFFFF_FFFF),
                CsrOp::ReadSet => (0xFFFF_FFFF, src_value),
                CsrOp::ReadClear => (0x0000_0000, src_value),
            };
            self.core
                .write_csr(self.allocator, csr, privilege_level, value, mask)
                .map_err(|_| Exception::IllegalInstruction)?;
        }
        Ok(())
    }
}

fn increment_pc(registers: &mut Registers) {
    let pc = registers.pc_mut();
    *pc = pc.wrapping_add(4);
}
