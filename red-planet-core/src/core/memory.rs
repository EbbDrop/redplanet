use super::ConnectedCore;
use crate::bus::{Bus, PureAccessError};
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
                self.read(&mut buf, allocator, address).map(|()|
                    match E {
                        LITTLE_ENDIAN => $u::from_le_bytes(buf),
                        BIG_ENDIAN => $u::from_be_bytes(buf),
                        CORE_ENDIAN => match self.core.endianness() {
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
                self.read_pure(&mut buf, allocator, address).map(|()|
                    match E {
                        LITTLE_ENDIAN => $u::from_le_bytes(buf),
                        BIG_ENDIAN => $u::from_be_bytes(buf),
                        CORE_ENDIAN => match self.core.endianness() {
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
                    CORE_ENDIAN => match self.core.endianness() {
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
pub struct Memory<'c, A: Allocator, B: Bus<A>> {
    pub(super) core: &'c ConnectedCore<A, B>,
}

impl<'c, A: Allocator, B: Bus<A>> Memory<'c, A, B> {
    pub fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) -> Result<(), MemoryError> {
        let physical_address = self.access(address, buf.len())?;
        self.core.system_bus.read(buf, allocator, physical_address);
        Ok(())
    }

    pub fn read_pure(
        &self,
        buf: &mut [u8],
        allocator: &A,
        address: u32,
    ) -> Result<(), MemoryError> {
        let physical_address = self.access(address, buf.len())?;
        self.core
            .system_bus
            .read_pure(buf, allocator, physical_address)
            .map_err(|_: PureAccessError| MemoryError::EffectfulReadOnly)
    }

    pub fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) -> Result<(), MemoryError> {
        let physical_address = self.access(address, buf.len())?;
        self.core.system_bus.write(allocator, physical_address, buf);
        Ok(())
    }

    pub fn read_byte(&self, allocator: &mut A, address: u32) -> Result<u8, MemoryError> {
        let mut buf = [0];
        self.read(&mut buf, allocator, address).map(|()| buf[0])
    }

    pub fn read_byte_pure(&self, allocator: &A, address: u32) -> Result<u8, MemoryError> {
        let mut buf = [0];
        self.read_pure(&mut buf, allocator, address)
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

    /// Performs the necessary checks for an at `address` of `size` bytes.
    /// Translates the address from virtual to physical.
    fn access(&self, address: u32, size: usize) -> Result<u32, MemoryError> {
        let size = u32::try_from(size).map_err(|_| MemoryError::AccessFault)?;

        if !self.core.config.support_misaligned_memory_access
            && !Alignment::natural_for_size(size)
                .map(|alignment| alignment.is_aligned(address))
                // If `size` is not a power of two, then the access is always considered unaligned
                // TODO: maybe `Memory::read` and `Memory::write` shouldn't be exposed, then this
                //       could never happen
                .unwrap_or(false)
        {
            return Err(MemoryError::MisalignedAccess);
        }

        // TODO: address translation must be done here, as the bounds checking on the range might
        //       cause access errors
        Ok(self.core.translate_address(address))
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
