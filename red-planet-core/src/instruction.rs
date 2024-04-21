use crate::registers::Specifier;
use thiserror::Error;

/// Data structure that can hold any supported instruction in its decoded form.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Instruction {
    OpImm {
        op: RegImmOp,
        dest: Specifier,
        src: Specifier,
        immediate: i32,
    },
    OpShiftImm {
        op: RegShiftImmOp,
        dest: Specifier,
        src: Specifier,
        shift_amount_u5: u32,
    },
    Auipc {
        dest: Specifier,
        immediate: i32,
    },
    Lui {
        dest: Specifier,
        immediate: i32,
    },
    Op {
        op: RegRegOp,
        dest: Specifier,
        src1: Specifier,
        src2: Specifier,
    },
    Jal {
        dest: Specifier,
        offset: i32,
    },
    Jalr {
        dest: Specifier,
        base: Specifier,
        offset: i32,
    },
    Branch {
        condition: BranchCondition,
        src1: Specifier,
        src2: Specifier,
        offset: i32,
    },
    Load {
        width: LoadWidth,
        dest: Specifier,
        base: Specifier,
        offset: i32,
    },
    Store {
        width: StoreWidth,
        src: Specifier,
        base: Specifier,
        offset: i32,
    },
    Fence {
        predecessor: FenceOrderCombination,
        successor: FenceOrderCombination,
    },
    Ecall,
    Ebreak,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RegImmOp {
    Addi,
    Slti,
    Sltiu,
    Xori,
    Ori,
    Andi,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RegShiftImmOp {
    Slli,
    Srli,
    Srai,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RegRegOp {
    Add,
    Slt,
    Sltu,
    And,
    Or,
    Xor,
    Sll,
    Srl,
    Sub,
    Sra,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BranchCondition {
    Beq,
    Bne,
    Blt,
    Bltu,
    Bge,
    Bgeu,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LoadWidth {
    Lb,
    Lh,
    Lw,
    Lbu,
    Lhu,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum StoreWidth {
    Sb,
    Sh,
    Sw,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FenceOrderCombination {
    pub device_input: bool,
    pub device_output: bool,
    pub memory_reads: bool,
    pub memory_writes: bool,
}

impl Instruction {
    pub fn decode(raw_instruction: u32) -> Result<Self, DecodeError> {
        match opcode(raw_instruction).ok_or(DecodeError::UnsupportedOpcode)? {
            Opcode::OpImm => match i_funct(raw_instruction) {
                Some(op) => Ok(Self::OpImm {
                    op,
                    dest: rd(raw_instruction),
                    src: rs1(raw_instruction),
                    immediate: i_imm(raw_instruction),
                }),
                None => match i_shfunct(raw_instruction) {
                    Some(op) => Ok(Self::OpShiftImm {
                        op,
                        dest: rd(raw_instruction),
                        src: rs1(raw_instruction),
                        shift_amount_u5: shamt(raw_instruction),
                    }),
                    None => Err(DecodeError::IllegalInstruction),
                },
            },
            Opcode::Auipc => Ok(Self::Auipc {
                dest: rd(raw_instruction),
                immediate: u_imm(raw_instruction),
            }),
            Opcode::Lui => Ok(Self::Lui {
                dest: rd(raw_instruction),
                immediate: u_imm(raw_instruction),
            }),
            Opcode::Op => match r_funct(raw_instruction) {
                Some(op) => Ok(Self::Op {
                    op,
                    dest: rd(raw_instruction),
                    src1: rs1(raw_instruction),
                    src2: rs2(raw_instruction),
                }),
                None => Err(DecodeError::IllegalInstruction),
            },
            Opcode::Jal => Ok(Self::Jal {
                dest: rd(raw_instruction),
                offset: j_imm(raw_instruction),
            }),
            Opcode::Jalr => Ok(Self::Jalr {
                dest: rd(raw_instruction),
                base: rs1(raw_instruction),
                offset: i_imm(raw_instruction),
            }),
            Opcode::Branch => match b_funct(raw_instruction) {
                Some(condition) => Ok(Self::Branch {
                    condition,
                    src1: rs1(raw_instruction),
                    src2: rs2(raw_instruction),
                    offset: b_imm(raw_instruction),
                }),
                None => Err(DecodeError::IllegalInstruction),
            },
            Opcode::Load => match i_width(raw_instruction) {
                Some(width) => Ok(Self::Load {
                    width,
                    dest: rd(raw_instruction),
                    base: rs1(raw_instruction),
                    offset: i_imm(raw_instruction),
                }),
                None => Err(DecodeError::IllegalInstruction),
            },
            Opcode::Store => match s_width(raw_instruction) {
                Some(width) => Ok(Self::Store {
                    width,
                    src: rs2(raw_instruction),
                    base: rs1(raw_instruction),
                    offset: s_imm(raw_instruction),
                }),
                None => Err(DecodeError::IllegalInstruction),
            },
            Opcode::MiscMem => {
                match i_mem(raw_instruction) {
                    Some(mem_funct) => match mem_funct {
                        MemFunct::Fence => {
                            let fm = raw_instruction >> 28;
                            let rd = u8::from(rd(raw_instruction));
                            let rs1 = u8::from(rs1(raw_instruction));
                            if fm != 0b0000 || rd != 0 || rs1 != 0 {
                                // All unused fields in the FENCE instruction encoding are reserved
                                // for future use. According to the spec, they should be treated as
                                // normal fence instructions (with fm == 0b0000) for forward
                                // compatibility.
                                //
                                // Note that the current spec defines one more optional encoding
                                // that we don't support: FENCE.TSO, which is encoded by
                                // fm == 0b1000 && predecessor==0b0011 && successor==0b0011
                                // && rs1 == 0 && rd == 0. The spec states this must be treated as
                                // "reserved for future use" if not supported, which again means
                                // treating it as a normal fence instruction (with fm == 0b0000) for
                                // forward compatibility.
                                //
                                // Therefore, there's nothing to be done here. No [`DecodeError`]
                                // that must be returned. We just continue with decoding the
                                // instruction as if fm == 0b0000 && rs1 == 0 && rd == 0.
                            }
                            let predecessor = FenceOrderCombination {
                                device_input: (raw_instruction >> 27) & 0b1 == 1,
                                device_output: (raw_instruction >> 26) & 0b1 == 1,
                                memory_reads: (raw_instruction >> 25) & 0b1 == 1,
                                memory_writes: (raw_instruction >> 24) & 0b1 == 1,
                            };
                            let successor = FenceOrderCombination {
                                device_input: (raw_instruction >> 23) & 0b1 == 1,
                                device_output: (raw_instruction >> 22) & 0b1 == 1,
                                memory_reads: (raw_instruction >> 21) & 0b1 == 1,
                                memory_writes: (raw_instruction >> 20) & 0b1 == 1,
                            };
                            Ok(Self::Fence {
                                predecessor,
                                successor,
                            })
                        }
                    },
                    None => Err(DecodeError::IllegalInstruction),
                }
            }
            Opcode::System => match i_sys(raw_instruction) {
                Some(sys) => Ok(match sys {
                    Sys::Ecall => Self::Ecall,
                    Sys::Ebreak => Self::Ebreak,
                }),
                None => Err(DecodeError::IllegalInstruction),
            },
        }
    }
}

// TODO: Create either more decode errors or join this in to one, because the current variants are
//       misleading! (i.e. they both indicate this is an unsupported encoding, which means it may
//       be reserved, not implemented, part of another extension, intended for a coprocessor, etc.)
#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum DecodeError {
    #[error("instruction has unsupported opcode")]
    UnsupportedOpcode,
    #[error("illegal instruction")]
    IllegalInstruction,
}

/// Returns the 7-bit *opcode* value of the instruction, or `None` if it isn't supported.
fn opcode(raw_instruction: u32) -> Option<Opcode> {
    #[allow(clippy::unusual_byte_groupings)]
    match raw_instruction & 0x7F {
        0b00_000_11 => Some(Opcode::Load),
        // LoadFp = 0b00_001_11,
        // custom-0
        0b00_011_11 => Some(Opcode::MiscMem),
        0b00_100_11 => Some(Opcode::OpImm),
        0b00_101_11 => Some(Opcode::Auipc),
        // OP-IMM-32
        // 48b
        0b01_000_11 => Some(Opcode::Store),
        // StoreFp = 0b01_001_11,
        // custom-1
        // Amo = 0b01_011_11,
        0b01_100_11 => Some(Opcode::Op),
        0b01_101_11 => Some(Opcode::Lui),
        // OP-32
        // 64b
        // Madd = 0b10_000_11,
        // Msub = 0b10_001_11,
        // Nmsub = 0b10_010_11,
        // Nmadd = 0b10_011_11,
        // OpFp = 0b10_100_11,
        // reserved
        // custom-2/rv128
        // 48b
        0b11_000_11 => Some(Opcode::Branch),
        0b11_001_11 => Some(Opcode::Jalr),
        // reserved
        0b11_011_11 => Some(Opcode::Jal),
        0b11_100_11 => Some(Opcode::System),
        // reserved
        // custom-3/rv128
        // >= 80b
        _ => None,
    }
}

/// Returns the 5-bit *rd* value for R-type, I-type, U-type, J-type instructions.
fn rd(raw_instruction: u32) -> Specifier {
    Specifier::from_u5(((raw_instruction >> 7) & 0x1F) as u8)
}

/// Returns the 5-bit *rs1* value for R-type, I-type, S-type, B-type instructions.
fn rs1(raw_instruction: u32) -> Specifier {
    Specifier::from_u5(((raw_instruction >> 15) & 0x1F) as u8)
}

/// Returns the 5-bit *rs2* value for R-type, S-type, B-type instructions.
fn rs2(raw_instruction: u32) -> Specifier {
    Specifier::from_u5(((raw_instruction >> 20) & 0x1F) as u8)
}

fn i_funct(raw_instruction: u32) -> Option<RegImmOp> {
    match funct3(raw_instruction) {
        0b000 => Some(RegImmOp::Addi),
        0b010 => Some(RegImmOp::Slti),
        0b011 => Some(RegImmOp::Sltiu),
        0b100 => Some(RegImmOp::Xori),
        0b110 => Some(RegImmOp::Ori),
        0b111 => Some(RegImmOp::Andi),
        _ => None,
    }
}

fn i_shfunct(raw_instruction: u32) -> Option<RegShiftImmOp> {
    let bit30 = (raw_instruction >> 30) & 1;
    match (bit30, funct3(raw_instruction)) {
        (0, 0b001) => Some(RegShiftImmOp::Slli),
        (0, 0b101) => Some(RegShiftImmOp::Srli),
        (1, 0b101) => Some(RegShiftImmOp::Srai),
        _ => None,
    }
}

fn i_sys(raw_instruction: u32) -> Option<Sys> {
    match (
        funct3(raw_instruction),
        i_imm(raw_instruction),
        u8::from(rs1(raw_instruction)),
        u8::from(rs2(raw_instruction)),
    ) {
        (0, 0, 0, 0) => Some(Sys::Ecall),
        (0, 1, 0, 0) => Some(Sys::Ebreak),
        _ => None,
    }
}

fn i_mem(raw_instruction: u32) -> Option<MemFunct> {
    match funct3(raw_instruction) {
        0b000 => Some(MemFunct::Fence),
        _ => None,
    }
}

fn i_width(raw_instruction: u32) -> Option<LoadWidth> {
    match funct3(raw_instruction) {
        0b000 => Some(LoadWidth::Lb),
        0b001 => Some(LoadWidth::Lh),
        0b010 => Some(LoadWidth::Lw),
        0b100 => Some(LoadWidth::Lbu),
        0b101 => Some(LoadWidth::Lhu),
        _ => None,
    }
}

fn s_width(raw_instruction: u32) -> Option<StoreWidth> {
    match funct3(raw_instruction) {
        0b000 => Some(StoreWidth::Sb),
        0b001 => Some(StoreWidth::Sh),
        0b010 => Some(StoreWidth::Sw),
        _ => None,
    }
}

fn r_funct(raw_instruction: u32) -> Option<RegRegOp> {
    match (funct7(raw_instruction), funct3(raw_instruction)) {
        (0b0000000, 0b000) => Some(RegRegOp::Add),
        (0b0000000, 0b001) => Some(RegRegOp::Sll),
        (0b0000000, 0b010) => Some(RegRegOp::Slt),
        (0b0000000, 0b011) => Some(RegRegOp::Sltu),
        (0b0000000, 0b100) => Some(RegRegOp::Xor),
        (0b0000000, 0b101) => Some(RegRegOp::Srl),
        (0b0000000, 0b110) => Some(RegRegOp::Or),
        (0b0000000, 0b111) => Some(RegRegOp::And),
        (0b0100000, 0b000) => Some(RegRegOp::Sub),
        (0b0100000, 0b101) => Some(RegRegOp::Sra),
        _ => None,
    }
}

fn b_funct(raw_instruction: u32) -> Option<BranchCondition> {
    match funct3(raw_instruction) {
        0b000 => Some(BranchCondition::Beq),
        0b001 => Some(BranchCondition::Bne),
        0b100 => Some(BranchCondition::Blt),
        0b101 => Some(BranchCondition::Bge),
        0b110 => Some(BranchCondition::Bltu),
        0b111 => Some(BranchCondition::Bgeu),
        _ => None,
    }
}

/// Returns the 3-bit *funct3* value for R-type, I-type, S-type, B-type instructions.
fn funct3(raw_instruction: u32) -> u8 {
    ((raw_instruction >> 12) & 0b111) as u8
}

/// Returns the 7-bit *funct7* value for R-type instructions.
fn funct7(raw_instruction: u32) -> u8 {
    (raw_instruction >> 25) as u8
}

/// Returns the 5-bit *shamt* value for S-type shift instructions.
fn shamt(raw_instruction: u32) -> u32 {
    (raw_instruction >> 20) & 0x1F
}

/// Returns the 12-bit I-immediate sign-extended to 32 bits.
fn i_imm(raw_instruction: u32) -> i32 {
    raw_instruction as i32 >> 20
}

/// Returns the 12-bit S-immediate sign-extended to 32 bits.
fn s_imm(raw_instruction: u32) -> i32 {
    let imm_11_5 = raw_instruction & 0x7F00_0000;
    let imm_4_0 = raw_instruction & 0x0000_0F80;
    (imm_11_5 | (imm_4_0 << 13)) as i32 >> 20
}

/// Returns the 13-bit B-immediate sign-extended to 32 bits.
fn b_imm(raw_instruction: u32) -> i32 {
    let imm_12 = raw_instruction & 0x8000_0000;
    let imm_10_5 = raw_instruction & 0x7E00_0000;
    let imm_4_1 = raw_instruction & 0x0000_0F00;
    let imm_11 = raw_instruction & 0x0000_0080;
    (imm_12 | (imm_11 << 23) | (imm_10_5 >> 1) | (imm_4_1 << 12)) as i32 >> 19
}

/// Returns the signed 32-bit U-immediate.
fn u_imm(raw_instruction: u32) -> i32 {
    (raw_instruction & 0xFFFF_F000) as i32
}

/// Returns the 21-bit J-immediate sign-extended to 32 bits.
fn j_imm(raw_instruction: u32) -> i32 {
    let imm_20 = raw_instruction & 0x8000_0000;
    let imm_10_1 = raw_instruction & 0x7FE0_0000;
    let imm_11 = raw_instruction & 0x0010_0000;
    let imm_19_12 = raw_instruction & 0x000F_F000;
    (imm_20 | (imm_19_12 << 11) | (imm_11 << 2) | (imm_10_1 >> 9)) as i32 >> 11
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Opcode {
    OpImm,
    Auipc,
    Lui,
    Op,
    Jal,
    Jalr,
    Branch,
    Load,
    Store,
    MiscMem,
    System,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Sys {
    Ecall,
    Ebreak,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum MemFunct {
    Fence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i_imm() {
        assert_eq!(0, i_imm(0x0000_0000));
        assert_eq!(-1, i_imm(0xFFF0_0000));
        assert_eq!(2047, i_imm(2047 << 20));
        assert_eq!(-2048, i_imm(0x8000_0000));
        assert_eq!(-42, i_imm((-42_i32 << 20) as u32));
        // Check other bits are ignored
        assert_eq!(0, i_imm(0x000F_FFFF));
        assert_eq!(-1, i_imm(0xFFF1_2345));
        assert_eq!(1209, i_imm((1209 << 20) | 0x000C_D10A));
    }
}
