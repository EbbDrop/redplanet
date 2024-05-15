use super::Core;
use crate::bus::PureAccessError;
use crate::system_bus::{AccessType, SystemBus};
use crate::{Alignment, Allocator, Endianness};
use thiserror::Error;

macro_rules! access_fns {
    ( $( $read_fn:ident, $read_pure_fn:ident, $write_fn:ident => $u:ident ),* $(,)? ) => {
        $(
            /// Invoke a read for the specified address.
            ///
            /// The address doesn't need to be naturally aligned, but implementations may return
            /// [`AccessError::Misaligned`] on misaligned access attempts.
            pub fn $read_fn<const E: MemOpEndianness>(
                &self,
                allocator: &mut A,
                address: u32,
            ) -> Result<$u, MemoryError> {
                let mut buf = [0u8; std::mem::size_of::<$u>()];
                self.read(&mut buf, allocator, address, false).map(|()|
                    match E {
                        LITTLE_ENDIAN => $u::from_le_bytes(buf),
                        BIG_ENDIAN => $u::from_be_bytes(buf),
                        CORE_ENDIAN => match self.core.endianness(allocator) {
                            Endianness::LE => $u::from_le_bytes(buf),
                            Endianness::BE => $u::from_be_bytes(buf),
                        }
                        _ => unreachable!(),
                    }
                )
            }

            /// Invoke an effect-free read for the specified address.
            ///
            /// The address doesn't need to be naturally aligned, but implementations may return
            /// [`AccessError::Misaligned`] on misaligned access attempts.
            ///
            /// See [`Bus::read_pure`] for the difference between this method and its non-pure
            /// counterpart.
            pub fn $read_pure_fn<const E: MemOpEndianness>(
                &self,
                allocator: &A,
                address: u32,
            ) -> Result<$u, MemoryError> {
                let mut buf = [0u8; std::mem::size_of::<$u>()];
                self.read_pure(&mut buf, allocator, address, false).map(|()|
                    match E {
                        LITTLE_ENDIAN => $u::from_le_bytes(buf),
                        BIG_ENDIAN => $u::from_be_bytes(buf),
                        CORE_ENDIAN => match self.core.endianness(allocator) {
                            Endianness::LE => $u::from_le_bytes(buf),
                            Endianness::BE => $u::from_be_bytes(buf),
                        }
                        _ => unreachable!(),
                    }
                )
            }

            /// Invoke a write for the specified address.
            ///
            /// The address doesn't need to be naturally aligned, but implementations may return
            /// [`AccessError::Misaligned`] on misaligned access attempts.
            pub fn $write_fn<const E: MemOpEndianness>(
                &self,
                allocator: &mut A,
                address: u32,
                value: $u,
            ) -> Result<(), MemoryError> {
                let buf = match E {
                    LITTLE_ENDIAN => value.to_le_bytes(),
                    BIG_ENDIAN => value.to_be_bytes(),
                    CORE_ENDIAN => match self.core.endianness(allocator) {
                        Endianness::LE => value.to_le_bytes(),
                        Endianness::BE => value.to_be_bytes(),
                    }
                    _ => unreachable!(),
                };
                self.write(allocator, address, &buf)
            }
        )*
    };
}

pub type MemOpEndianness = u8;

/// The core's current endianness mode.
pub const CORE_ENDIAN: MemOpEndianness = 0;

/// Little-endian (least significant byte at lowest address).
pub const LITTLE_ENDIAN: MemOpEndianness = 1;

/// Big-endian (most significant byte at lowest address).
pub const BIG_ENDIAN: MemOpEndianness = 2;

/// Access wrapper around a raw bus to address it as memory from this core's point of view.
///
/// This is a continuous, circular, byte-addressable address space of `pow(2, 32)` bytes.
/// It is designed as a mapping of address ranges to (hardware) resources.
///
/// This takes into account the core's current privilege level, its memory mapping (i.e. which
/// regions can be accessed), its configuration (e.g. whether misaligned memory accesses are
/// supported), etc.
#[derive(Debug, Clone)]
pub struct Mmu<'c, A: Allocator, B: SystemBus<A>> {
    pub(super) core: &'c Core<A, B>,
}

impl<'c, A: Allocator, B: SystemBus<A>> Mmu<'c, A, B> {
    pub fn read_byte(&self, allocator: &mut A, address: u32) -> Result<u8, MemoryError> {
        let mut buf = [0];
        self.read(&mut buf, allocator, address, false)
            .map(|()| buf[0])
    }

    pub fn read_byte_pure(&self, allocator: &A, address: u32) -> Result<u8, MemoryError> {
        let mut buf = [0];
        self.read_pure(&mut buf, allocator, address, false)
            .map(|()| buf[0])
    }

    pub fn write_byte(
        &self,
        allocator: &mut A,
        address: u32,
        value: u8,
    ) -> Result<(), MemoryError> {
        self.write(allocator, address, &[value])
    }

    access_fns! {
        read_halfword, read_halfword_pure, write_halfword => u16,
        read_word, read_word_pure, write_word => u32,
        read_doubleword, read_doubleword_pure, write_doubleword => u64,
        read_quadword, read_quadword_pure, write_quadword => u128,
    }

    /// Reads a naturally-aligned 32-bit little-endian word from memory.
    ///
    /// > The base RISC-V ISA has fixed-length 32-bit instructions that must be naturally aligned on
    /// > 32-bit boundaries.
    ///
    /// > Instructions are stored in memory as a sequence of 16-bit little-endian parcels,
    /// > regardless of memory system endianness. Parcels forming one instruction are stored at
    /// > increasing halfword addresses, with the lowest-addressed parcel holding the
    /// > lowest-numbered bits in the instruction specification.
    pub fn fetch_instruction(&self, allocator: &mut A, address: u32) -> Result<u32, MemoryError> {
        if !Alignment::WORD.is_aligned(address) {
            return Err(MemoryError::MisalignedAccess);
        }
        let mut buf = [0u8; 4];
        self.read(&mut buf, allocator, address, true)
            .map(|()| u32::from_le_bytes(buf))
    }

    fn read(
        &self,
        buf: &mut [u8],
        allocator: &mut A,
        address: u32,
        execute: bool,
    ) -> Result<(), MemoryError> {
        let access_type = match execute {
            true => AccessType::Execute,
            false => AccessType::Read,
        };
        let physical_address = self.access(address, buf.len(), access_type)?;
        self.core.system_bus.read(buf, allocator, physical_address);
        Ok(())
    }

    fn read_pure(
        &self,
        buf: &mut [u8],
        allocator: &A,
        address: u32,
        execute: bool,
    ) -> Result<(), MemoryError> {
        let access_type = match execute {
            true => AccessType::Execute,
            false => AccessType::Read,
        };
        let physical_address = self.access(address, buf.len(), access_type)?;
        self.core
            .system_bus
            .read_pure(buf, allocator, physical_address)
            .map_err(|_: PureAccessError| MemoryError::EffectfulReadOnly)
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) -> Result<(), MemoryError> {
        let physical_address = self.access(address, buf.len(), AccessType::Write)?;
        self.core.system_bus.write(allocator, physical_address, buf);
        Ok(())
    }

    /// Performs the necessary checks for an at `address` of `size` bytes.
    /// Translates the address from virtual to physical.
    fn access(
        &self,
        address: u32,
        size: usize,
        access_type: AccessType,
    ) -> Result<u32, MemoryError> {
        let size = u32::try_from(size).map_err(|_| MemoryError::AccessFault)?;

        if !self.core.config.support_misaligned_memory_access
            && !Alignment::natural_for_size(size)
                .map(|alignment| alignment.is_aligned(address))
                // If `size` is not a power of two, then the access is always considered unaligned
                .unwrap_or(false)
        {
            return Err(MemoryError::MisalignedAccess);
        }

        let physical_address = self.core.translate_address(address);

        if self
            .core
            .system_bus
            .accepts(physical_address, size as usize, access_type)
        {
            Ok(physical_address)
        } else {
            Err(MemoryError::AccessFault)
        }
    }
}

#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum MemoryError {
    #[error("misaligned access")]
    MisalignedAccess,
    #[error("access fault")]
    AccessFault,
    #[error("cannot read effect-free")]
    EffectfulReadOnly,
}
