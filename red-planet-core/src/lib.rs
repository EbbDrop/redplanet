#[macro_use]
extern crate static_assertions;

use std::cmp::Ordering;
use std::fmt;
use thiserror::Error;

pub mod address_map;
pub mod address_range;
pub mod board;
pub mod bus;
pub mod core;
pub mod instruction;
pub mod interrupt;
pub mod registers;
pub mod resources;
pub mod simulator;
pub mod system_bus;

// Re-export Allocator trait so dependants don't need to include space-time as a dependency
/// Trait for types that can store state of simulated components.
///
pub use space_time::allocator::{Allocator, ArrayAccessor, ArrayAccessorMut};

/// Re-export of [`AddressRange`] for convenience.
pub use address_range::AddressRange;

/// List of all possible privilege levels for RISC-V.
///
/// Same as [`PrivilegeLevel`] except that it allows specifying the reserved privilege level `2`.
/// This can be useful in case a minimum required privilege level is specified as a 2-bit value,
/// since that value itself may be a reserved privilege level.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum RawPrivilegeLevel {
    User = 0,
    Supervisor = 1,
    /// Privilege level `0b10` is reserved in the base ISA. When using the hypervisor extension,
    /// this becomes the Hypervisor privilege level.
    Reserved = 2,
    Machine = 3,
}

impl RawPrivilegeLevel {
    /// Convert a 2-bit value into a [`RawPrivilegeLevel`].
    /// Panics if the value doesn't fit in 2 bits (`0..=3`).
    pub fn from_u2(value_u2: u8) -> Self {
        match value_u2 {
            0 => Self::User,
            1 => Self::Supervisor,
            2 => Self::Reserved,
            3 => Self::Machine,
            _ => panic!("out of range u2 used"),
        }
    }

    pub fn is_reserved(self) -> bool {
        matches!(self, Self::Reserved)
    }
}

impl fmt::Display for RawPrivilegeLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            RawPrivilegeLevel::User => "U",
            RawPrivilegeLevel::Supervisor => "S",
            RawPrivilegeLevel::Reserved => "2",
            RawPrivilegeLevel::Machine => "M",
        })
    }
}

/// List of defined privilege levels for RISC-V.
///
/// A privilege level is always referenced by two bits, so only `0`, `1`, `2`, and `3` are valid
/// privilege levels. However, only levels `0`, `1`, and `3` are defined; level `2` is considered
/// *reserved* for now.
///
/// > The machine level has the highest privileges and is the only mandatory privilege level for a
/// > RISC-V hardware platform. Code run in machine-mode (M-mode) is usually inherently trusted, as
/// > it has low-level access to the machine implementation. M-mode can be used to manage secure
/// > execution environments on RISC-V. User-mode (U-mode) and supervisor-mode (S-mode) are intended
/// > for conventional application and operating system usage respectively.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum PrivilegeLevel {
    /// User/application (abbreviated `U`) is the lower privilege level.
    User = 0,
    /// Supervisor (abbreviated `S`) is an intermediate privilege level,
    /// that allows protection from OS.
    Supervisor = 1,
    // Level 2 is reserved
    /// Machine (abbreviated `M`) is the highest privilege level.
    /// It is the only mandatory privilege level for a RISC-V hardware platform.
    Machine = 3,
}

impl PartialEq<PrivilegeLevel> for RawPrivilegeLevel {
    fn eq(&self, other: &PrivilegeLevel) -> bool {
        *self as usize == *other as usize
    }
}

impl PartialEq<RawPrivilegeLevel> for PrivilegeLevel {
    fn eq(&self, other: &RawPrivilegeLevel) -> bool {
        *self as usize == *other as usize
    }
}

impl PartialOrd<PrivilegeLevel> for RawPrivilegeLevel {
    fn partial_cmp(&self, other: &PrivilegeLevel) -> Option<Ordering> {
        (*self as usize).partial_cmp(&(*other as usize))
    }
}

impl PartialOrd<RawPrivilegeLevel> for PrivilegeLevel {
    fn partial_cmp(&self, other: &RawPrivilegeLevel) -> Option<Ordering> {
        (*self as usize).partial_cmp(&(*other as usize))
    }
}

impl fmt::Display for PrivilegeLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            PrivilegeLevel::User => "U",
            PrivilegeLevel::Supervisor => "S",
            PrivilegeLevel::Machine => "M",
        })
    }
}

impl From<PrivilegeLevel> for RawPrivilegeLevel {
    fn from(value: PrivilegeLevel) -> Self {
        match value {
            PrivilegeLevel::User => Self::User,
            PrivilegeLevel::Supervisor => Self::Supervisor,
            PrivilegeLevel::Machine => Self::Machine,
        }
    }
}

impl TryFrom<RawPrivilegeLevel> for PrivilegeLevel {
    type Error = ReservedPrivilegeLevelError;
    fn try_from(value: RawPrivilegeLevel) -> Result<Self, Self::Error> {
        match value {
            RawPrivilegeLevel::User => Ok(Self::User),
            RawPrivilegeLevel::Supervisor => Ok(Self::Supervisor),
            RawPrivilegeLevel::Reserved => Err(ReservedPrivilegeLevelError(value)),
            RawPrivilegeLevel::Machine => Ok(Self::Machine),
        }
    }
}

#[derive(Error, Debug)]
#[error("privilege level {0} is reserved")]
pub struct ReservedPrivilegeLevelError(RawPrivilegeLevel);

pub mod unit {
    //! Collection of the units in which memory can be addressed (in bytes).

    /// A _byte_ is 8 bits.
    pub const BYTE: u32 = 1;

    /// A _halfword_ is 16 bits (2 bytes).
    pub const HALFWORD: u32 = 2;

    /// A _word_ is 32 bits (4 bytes).
    pub const WORD: u32 = 4;

    /// A _doubleword_ is 64 bits (8 bytes).
    pub const DOUBLEWORD: u32 = 8;

    /// A _quadword_ is 128 bits (16 bytes).
    pub const QUADWORD: u32 = 16;
}

/// Address alignment ranging from no alignment (`1`) to `1 << 31` alignment.
/// Representing `1 << 32` alignment is possible by specifying an alignment of `0`.
// Maintains the invariant that self.0 is a power of two, or 0.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Alignment(u32);

impl Alignment {
    //! Collection of the natural alignments (as exponents) for each unit in which memory can be
    //! addressed.

    /// Byte alignment is equivalent to no alignment.
    pub const BYTE: Self = Self(1);

    /// Halfword alignment means the address is a multiple of 2 (`address & 0b1 == 0`).
    pub const HALFWORD: Self = Self(2);

    /// Word alignment means the address is a multiple of 4 (`address & 0b11 == 0`).
    pub const WORD: Self = Self(4);

    /// Doubleword alignment means the address is a multiple of 8 (`address & 0b111 == 0`).
    pub const DOUBLEWORD: Self = Self(8);

    /// Quadword alignment means the address is a multiple of 16 (`address & 0b1111 == 0`).
    pub const QUADWORD: Self = Self(16);

    /// `1 << 32` alignment means the address can only be `0`.
    pub const MAX: Self = Self(0);

    /// Creates the natural alignment for a unit of size `size`. Returns `None` if `size` is not a
    /// multiple of two, except if it is `0`, in which case `1 << 32` alignment is returned.
    ///
    /// If `size` is a multiple of two, the alignment will be equal to the size.
    pub fn natural_for_size(size: u32) -> Option<Self> {
        if size == 0 {
            Some(Self(0))
        } else {
            size.is_power_of_two().then_some(Self(size))
        }
    }

    /// Returns the alignment corresponding to `1 << exponent`.
    /// No alignment greater than `1 << 32` can be represented, hence `None` will be returned if
    /// `exponent > 32`.
    pub fn from_exponent(exponent: u8) -> Option<Self> {
        (exponent <= 32).then_some(Self(1u32.wrapping_shl(exponent as u32)))
    }

    /// Returns the alignment corresponding to the power of two passed in.
    /// Returns `None` if `power_of_two` is not a power of two, except if it is `0`, in which case
    /// an alignment of `1 << 32` is returned.
    pub fn from_power_of_two(power_of_two: u32) -> Option<Self> {
        if power_of_two == 0 {
            Some(Self(0))
        } else {
            power_of_two.is_power_of_two().then_some(Self(power_of_two))
        }
    }

    /// Returns the alignment as a power of two, modulo `1 << 32`.
    /// This means an alignment of `1 << 32` will return `0`.
    pub fn as_power_of_two(self) -> u32 {
        self.0
    }

    /// Returns the exponent of two needed to reach the power of two this alignment represents.
    ///
    /// For example, for quadword alignment `self.as_power_of_two() == 8`, while
    /// `self.as_exponent() == 3`.
    ///
    /// This will always return a value in the range `0..=32`.
    pub fn as_exponent(self) -> u8 {
        self.0.checked_ilog2().unwrap_or(32) as u8
    }

    /// Returns `true` if `address` is aligned to this alignment.
    pub fn is_aligned(self, address: u32) -> bool {
        address & self.0.wrapping_sub(1) == 0
    }
}

/// Sum type for the two possible byte orders: big-endian or little-endian.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Endianness {
    /// Little-endian (least significant byte at lowest address)
    LE,
    /// Big-endian (most significant byte at lowest address)
    BE,
}

/// Wrapper around [`Allocator`] for single objects of type `T` that are never deallocated during
/// the lifetime of this wrapper.
///
/// The primary goal of this wrapper is to provide a more convenient interface around
/// [`Allocator::get`] and [`Allocator::get_mut`], which does return a (mutable) reference directly
/// rather than a `Result`.
#[derive(Debug)]
pub struct Allocated<A: Allocator, T: 'static + Clone>(A::Id<T>);

impl<A: Allocator, T: 'static + Clone> Allocated<A, T> {
    /// Inserts `object` into `allocator`. See also [`Allocator::insert`].
    pub fn new(allocator: &mut A, object: T) -> Self {
        Self(allocator.insert(object))
    }

    /// Pops the inner object from `allocator`, returning it while consuming `self`.
    /// See also [`Allocator::pop`].
    ///
    /// # Panics
    ///
    /// Panics if the inner object was already removed from `allocator`.
    pub fn into_inner(self, allocator: &mut A) -> T {
        allocator.pop(self.0).unwrap()
    }

    /// Removes the inner object from `allocator`, consuming `self`. See also [`Allocator::remove`].
    ///
    /// If you need an owned version of the inner object, use [`into_inner`](Self::into_inner).
    ///
    /// # Panics
    ///
    /// Panics if the inner object was already removed from `allocator`.
    pub fn drop(self, allocator: &mut A) {
        allocator.remove(self.0).unwrap()
    }

    /// Returns a reference to the stored object. See also [`Allocator::get`].
    ///
    /// # Panics
    ///
    /// Panics if the inner object has been removed from `allocator`.
    pub fn get<'a>(&self, allocator: &'a A) -> &'a T {
        allocator.get(self.0).unwrap()
    }

    /// Returns a mutable reference to the stored object. See also [`Allocator::get_mut`].
    ///
    /// # Panics
    ///
    /// Panics if the inner object has been removed from `allocator`.
    pub fn get_mut<'a>(&self, allocator: &'a mut A) -> &'a mut T {
        allocator.get_mut(self.0).unwrap()
    }
}
