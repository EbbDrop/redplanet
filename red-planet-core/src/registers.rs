//! General purpose registers, unallocated.

use core::fmt;
use std::fmt::Formatter;

/// The type of a single `x` register.
pub type X = u32;

/// The bit width of the `x` registers.
pub const XLEN: u32 = X::BITS;

/// The number of `x` registers available (indices start at `0` for `x0`)
pub const LEN: u8 = 32;

/// A RISC-V core's general purpose registers.
///
/// There are 32 `x` word-size (32 bit) registers, named `x0` up to `x31`.
/// The register `x0` (aka `zero`) is always zero. Writes to it are ignored.
/// There is also the `pc` register which holds the Program Counter (also 32 bits).
///
/// > For RV32I, the 32 x registers are each 32 bits wide, i.e., XLEN=32. Register x0 is hardwired
/// > with all bits equal to 0. General purpose registers x1–x31 hold values that various
/// > instructions interpret as a collection of Boolean values, or as two’s complement signed binary
/// > integers or unsigned binary integers.
/// >
/// > There is one additional unprivileged register: the program counter pc holds the address of the
/// > current instruction.
///
///
/// It is not possible to get a mutable reference to an `x` register, since that would allow
/// unchecked writes to register `x0`.
#[derive(Debug, Clone)]
pub struct Registers {
    x_registers: [X; LEN as usize],
    pc: u32,
}

impl Default for Registers {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Registers {
    /// Returns a fresh set of all-zero registers.
    pub fn new(initial_pc: u32) -> Self {
        Self {
            x_registers: [0; LEN as usize],
            pc: initial_pc,
        }
    }

    /// Returns the value of an `x` register.
    pub fn x(&self, specifier: Specifier) -> u32 {
        self.x_registers[usize::from(specifier)]
    }

    /// Sets the value of an `x` register.
    ///
    /// Writes to register `x0` are ignored.
    pub fn set_x(&mut self, specifier: Specifier, value: u32) {
        self.replace_x(specifier, value);
    }

    /// Replaces the value of an `x` register, returning its old value.
    ///
    /// Writes to register `x0` are ignored.
    pub fn replace_x(&mut self, specifier: Specifier, value: u32) -> u32 {
        if specifier.0 == 0 {
            0 // Ignore writes to register `x0`
        } else {
            std::mem::replace(&mut self.x_registers[specifier.0 as usize], value)
        }
    }

    /// Returns the value of the `pc` register.
    pub fn pc(&self) -> u32 {
        self.pc
    }

    /// Returns a mutable reference to the `pc` register value.
    pub fn pc_mut(&mut self) -> &mut u32 {
        &mut self.pc
    }
}

/// An `x` register specifier. Can take values in the range `0..LEN`.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Specifier(u8);

impl Specifier {
    /// Register `x0`, a.k.a. register `zero`, always returns `0` on read, and ignores any writes.
    pub const X0: Self = Specifier(0);

    /// Create a register specifier from its index, returning `None` if `index > 31`.
    pub fn new<U: TryInto<u8>>(index: U) -> Option<Self> {
        let index = index.try_into().ok()?;
        (index < 32).then_some(Self(index))
    }

    /// Convert a 5-bit value into a register specifier.
    /// Panics if the value doesn't fit in 5 bits (`0..=31`).
    pub fn from_u5(value_u5: u8) -> Self {
        const_assert_eq!(LEN, 32);
        if value_u5 > 31 {
            panic!("out of range u5 used");
        }
        Self(value_u5)
    }

    /// Return an iterator over all register specifier, starting at x0 up to x31.
    pub fn iter_all() -> impl Iterator<Item = Self> {
        (0..32).map(Self)
    }
}

impl From<Specifier> for u8 {
    fn from(value: Specifier) -> Self {
        value.0
    }
}

impl From<Specifier> for u32 {
    fn from(value: Specifier) -> Self {
        value.0 as u32
    }
}

impl From<Specifier> for usize {
    fn from(value: Specifier) -> Self {
        value.0 as usize
    }
}

impl fmt::Display for Specifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "x{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(32, XLEN);
        const_assert!(LEN > 1);
    }

    #[test]
    fn test_write_to_zero() {
        let mut registers = Registers::default();
        assert_eq!(0, registers.x(Specifier::X0));
        assert_eq!(0, registers.pc());
        registers.set_x(Specifier::X0, 0xDEADBEEF);
        assert_eq!(0, registers.x(Specifier::X0));
        assert_eq!(0, registers.pc());
    }

    #[test]
    fn test_write_to_pc() {
        let mut registers = Registers::default();
        assert_eq!(0, registers.pc());
        assert_eq!(0, registers.x(Specifier::X0));
        *registers.pc_mut() = 0xDEADBEEF;
        assert_eq!(0xDEADBEEF, registers.pc());
        assert_eq!(0, registers.x(Specifier::X0));
    }

    #[test]
    fn test_get_x() {
        let registers = Registers::default();
        for i in 0..LEN {
            assert_eq!(0, registers.x(Specifier::from_u5(i)));
        }
    }

    #[test]
    fn test_set_x() {
        let mut registers = Registers::default();
        registers.set_x(Specifier::X0, 1);
        for i in 1..LEN {
            registers.set_x(Specifier::from_u5(i), i as u32 + 1);
        }
        assert_eq!(0, registers.x(Specifier::X0));
        for i in 1..LEN {
            assert_eq!(i as u32 + 1, registers.x(Specifier::from_u5(i)));
        }
    }

    #[test]
    fn test_replace_x() {
        let mut registers = Registers::default();
        assert_eq!(0, registers.replace_x(Specifier::X0, 0));
        for i in 1..LEN {
            assert_eq!(0, registers.replace_x(Specifier::from_u5(i), i as u32));
        }
        assert_eq!(0, registers.replace_x(Specifier::X0, 1));
        for i in 1..LEN {
            assert_eq!(
                i as u32,
                registers.replace_x(Specifier::from_u5(i), i as u32 + 1)
            );
        }
        assert_eq!(0, registers.x(Specifier::X0));
        for i in 1..LEN {
            assert_eq!(i as u32 + 1, registers.x(Specifier::from_u5(i)));
        }
    }
}
