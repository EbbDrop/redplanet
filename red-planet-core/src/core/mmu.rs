use std::borrow::Borrow;

use super::trap::SatpMode;
use super::Core;
use crate::system_bus::{AccessType, SystemBus};
use crate::{Alignment, Allocator, Endianness, PrivilegeLevel};
use bitvec::field::BitField;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use log::{debug, trace};
use thiserror::Error;

macro_rules! access_fns {
    ( $( $read_fn:ident, $read_debug_fn:ident, $write_fn:ident => $u:ident ),* $(,)? ) => {
        $(
            /// Invoke a read for the specified address.
            pub fn $read_fn(&self, allocator: &mut A, address: u32) -> Result<$u, MemoryError> {
                trace!("Reading {} from memory at vaddr {address:#010x}", stringify!($u));
                let privilege_level = self.core.effective_privilege_mode(allocator);
                let mut buf = [0u8; std::mem::size_of::<$u>()];
                self.read(&mut buf, allocator, address, privilege_level, false)?;
                Ok(match self.core.endianness(allocator, privilege_level) {
                    Endianness::LE => $u::from_le_bytes(buf),
                    Endianness::BE => $u::from_be_bytes(buf),
                })
            }

            /// Perform a debug read for the specified address.
            ///
            /// See [`Bus::read_debug`](crate::bus::Bus::read_debug) for the difference between this
            /// method and its non-debug counterpart.
            pub fn $read_debug_fn(&self, allocator: &A, address: u32) -> Result<$u, MemoryError> {
                trace!("Debug reading {} from memory at vaddr {address:#010x}", stringify!($u));
                let privilege_level = self.core.effective_privilege_mode(allocator);
                let mut buf = [0u8; std::mem::size_of::<$u>()];
                self.read_debug(&mut buf, allocator, address, privilege_level, false)?;
                Ok(match self.core.endianness(allocator, privilege_level) {
                    Endianness::LE => $u::from_le_bytes(buf),
                    Endianness::BE => $u::from_be_bytes(buf),
                })
            }

            /// Invoke a write for the specified address.
            pub fn $write_fn(
                &self,
                allocator: &mut A,
                address: u32,
                value: $u,
            ) -> Result<(), MemoryError> {
                trace!(value; "Writing {} to memory at vaddr {address:#010x}", stringify!($u));
                let privilege_level = self.core.effective_privilege_mode(allocator);
                let buf = match self.core.endianness(allocator, privilege_level) {
                    Endianness::LE => value.to_le_bytes(),
                    Endianness::BE => value.to_be_bytes(),
                };
                self.write(allocator, address, &buf, privilege_level)
            }
        )*
    };
}

const PAGE_TABLE_LEVELS: u32 = 2;
// log2(Size of a single page (in bytes))
const PAGE_SIZE_SHF: u32 = 12;
// log2(Size of a single PTE (in bytes))
const PTE_SIZE_SHF: u32 = 2;

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
        trace!("Reading byte from memory at vaddr {address:#010x}");
        let privilege_level = self.core.effective_privilege_mode(allocator);
        let mut buf = [0];
        self.read(&mut buf, allocator, address, privilege_level, false)
            .map(|()| buf[0])
    }

    pub fn read_byte_debug(&self, allocator: &A, address: u32) -> Result<u8, MemoryError> {
        trace!("Debug reading byte from memory at vaddr {address:#010x}");
        let privilege_level = self.core.effective_privilege_mode(allocator);
        let mut buf = [0];
        self.read_debug(&mut buf, allocator, address, privilege_level, false)
            .map(|()| buf[0])
    }

    pub fn write_byte(
        &self,
        allocator: &mut A,
        address: u32,
        value: u8,
    ) -> Result<(), MemoryError> {
        trace!(value; "Writing byte to memory at vaddr {address:#010x}");
        let privilege_level = self.core.effective_privilege_mode(allocator);
        self.write(allocator, address, &[value], privilege_level)
    }

    access_fns! {
        read_halfword, read_halfword_debug, write_halfword => u16,
        read_word, read_word_debug, write_word => u32,
        read_doubleword, read_doubleword_debug, write_doubleword => u64,
        read_quadword, read_quadword_debug, write_quadword => u128,
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
        trace!("Fetching instruction from memory at vaddr {address:#010x}");
        let alignment = match self.core.config.strict_instruction_alignment {
            true => Alignment::WORD,
            false => Alignment::HALFWORD,
        };
        if !alignment.is_aligned(address) {
            debug!("Failed to fetch instruction: address misaligned: {address:#010x}");
            return Err(MemoryError::MisalignedAccess);
        }
        // Use the core's current privilege level, not its *effective* privilege level, since that
        // shouldn't be used for instruction fetches.
        let privilege_level = self.core.privilege_mode(allocator);
        let mut buf = [0u8; 4];
        self.read(&mut buf, allocator, address, privilege_level, true)
            .map(|()| u32::from_le_bytes(buf))
    }

    pub fn read_range(
        &self,
        buf: &mut [u8],
        allocator: &mut A,
        address: u32,
    ) -> Result<(), MemoryError> {
        let privilege_level = self.core.privilege_mode(allocator);
        self.read_debug(buf, allocator, address, privilege_level, false)
    }

    pub fn read_range_debug(
        &self,
        buf: &mut [u8],
        allocator: &A,
        address: u32,
    ) -> Result<(), MemoryError> {
        let privilege_level = self.core.privilege_mode(allocator);
        self.read_debug(buf, allocator, address, privilege_level, false)
    }

    pub fn write_range(
        &self,
        allocator: &mut A,
        address: u32,
        buf: &[u8],
    ) -> Result<(), MemoryError> {
        let privilege_level = self.core.privilege_mode(allocator);
        self.write(allocator, address, buf, privilege_level)
    }

    fn read(
        &self,
        buf: &mut [u8],
        allocator: &mut A,
        address: u32,
        privilege_level: PrivilegeLevel,
        execute: bool,
    ) -> Result<(), MemoryError> {
        let access_type = match execute {
            true => AccessType::Execute,
            false => AccessType::Read,
        };
        let physical_address =
            self.access_virtual(allocator, address, buf.len(), access_type, privilege_level)?;
        self.core.system_bus.read(buf, allocator, physical_address);
        Ok(())
    }

    fn read_debug(
        &self,
        buf: &mut [u8],
        allocator: &A,
        address: u32,
        privilege_level: PrivilegeLevel,
        execute: bool,
    ) -> Result<(), MemoryError> {
        let access_type = match execute {
            true => AccessType::Execute,
            false => AccessType::Read,
        };
        let physical_address =
            self.access_virtual_debug(allocator, address, buf.len(), access_type, privilege_level)?;
        self.core
            .system_bus
            .read_debug(buf, allocator, physical_address);
        Ok(())
    }

    fn write(
        &self,
        allocator: &mut A,
        address: u32,
        buf: &[u8],
        privilege_level: PrivilegeLevel,
    ) -> Result<(), MemoryError> {
        let physical_address = self.access_virtual(
            allocator,
            address,
            buf.len(),
            AccessType::Write,
            privilege_level,
        )?;
        self.core.system_bus.write(allocator, physical_address, buf);
        Ok(())
    }

    /// Performs the necessary checks for access virtual `address` of `size` bytes.
    /// Translates the address from virtual to physical.
    fn access_virtual(
        &self,
        allocator: &mut A,
        address: u32,
        size: usize,
        access_type: AccessType,
        privilege_level: PrivilegeLevel,
    ) -> Result<u32, MemoryError> {
        self.access_virtual_pre_translate_checks(address, size, access_type)?;
        let physical_address =
            self.translate_address(allocator, address, access_type, privilege_level)?;
        self.access_physical(physical_address, size, access_type)?;
        Ok(physical_address)
    }

    /// Performs the necessary checks for access virtual `address` of `size` bytes.
    /// Translates the address from virtual to physical.
    fn access_virtual_debug(
        &self,
        allocator: &A,
        address: u32,
        size: usize,
        access_type: AccessType,
        privilege_level: PrivilegeLevel,
    ) -> Result<u32, MemoryError> {
        self.access_virtual_pre_translate_checks(address, size, access_type)?;
        let physical_address =
            self.translate_address_debug(allocator, address, access_type, privilege_level)?;
        self.access_physical(physical_address, size, access_type)?;
        Ok(physical_address)
    }

    fn access_virtual_pre_translate_checks(
        &self,
        address: u32,
        size: usize,
        access_type: AccessType,
    ) -> Result<(), MemoryError> {
        let size = u32::try_from(size).map_err(|_| MemoryError::AccessFault)?;

        if !self.core.config.support_misaligned_memory_access
            && !Alignment::natural_for_size(size)
                .map(|alignment| alignment.is_aligned(address))
                // If `size` is not a power of two, then the access is always considered unaligned
                .unwrap_or(false)
        {
            debug!(
                address, size, access_type:%,
                core_supports_misaligned_accesses=self.core.config.support_misaligned_memory_access;
                "Memory access misaligned"
            );
            return Err(MemoryError::MisalignedAccess);
        }

        Ok(())
    }

    // Perform PMA & PMP checks for physical (`address`, `size`) accesses of type `access_type`.
    fn access_physical(
        &self,
        address: u32,
        size: usize,
        access_type: AccessType,
    ) -> Result<(), MemoryError> {
        // TODO: PMP checks
        if self.core.system_bus.accepts(address, size, access_type) {
            Ok(())
        } else {
            debug!(
                address, size, access_type:%;
                "Memory access not accepted by system bus"
            );
            Err(MemoryError::AccessFault)
        }
    }

    /// Map a virtual byte address to the corresponding physical byte address.
    fn translate_address(
        &self,
        allocator: &mut A,
        address: u32,
        access_type: AccessType,
        privilege_level: PrivilegeLevel,
    ) -> Result<u32, MemoryError> {
        self.translate_address_common(
            allocator,
            address,
            access_type,
            privilege_level,
            |allocator, entry_address| {
                self.read_pte(allocator, entry_address)
                    .map(|entry| (allocator, entry))
            },
            |allocator, address, value| self.write_pte(allocator, address, value),
        )
    }

    /// Map a virtual byte address to the corresponding physical byte address.
    fn translate_address_debug(
        &self,
        allocator: &A,
        address: u32,
        access_type: AccessType,
        privilege_level: PrivilegeLevel,
    ) -> Result<u32, MemoryError> {
        self.translate_address_common(
            allocator,
            address,
            access_type,
            privilege_level,
            |allocator, entry_address| {
                self.read_pte_debug(allocator, entry_address)
                    .map(|entry| (allocator, entry))
            },
            |_allocator, _address, _value| Ok(()),
        )
    }

    // Base implementation of [`Self::translate_address`] and [`Self::translate_address_debug`].
    fn translate_address_common<ARef: Borrow<A>>(
        &self,
        mut allocator: ARef,
        address: u32,
        access_type: AccessType,
        privilege_level: PrivilegeLevel,
        read_pte: impl Fn(ARef, u32) -> Result<(ARef, u32), MemoryError>,
        write_pte: impl Fn(ARef, u32, u32) -> Result<(), MemoryError>,
    ) -> Result<u32, MemoryError> {
        // Satp register must be active (effective privilege mode U or S).
        let user_mode = match privilege_level {
            PrivilegeLevel::Machine => return Ok(address),
            PrivilegeLevel::User => true,
            PrivilegeLevel::Supervisor => false,
        };
        let trap = self.core.trap.get(allocator.borrow());
        match trap.satp_mode() {
            SatpMode::Bare => return Ok(address),
            SatpMode::Sv32 => {}
        };
        const PAGE_SIZE_MSK: u32 = (1 << PAGE_SIZE_SHF) - 1;
        // log2(Number of PTEs that fit in one page)
        const PTE_COUNT_SHF: u32 = PAGE_SIZE_SHF - PTE_SIZE_SHF;
        const PTE_COUNT_MSK: u32 = (1 << PTE_COUNT_SHF) - 1;
        // STEP 1
        let mut page_table = trap.satp_ppn() << PAGE_SIZE_SHF;
        for level in (0..PAGE_TABLE_LEVELS).rev() {
            // STEP 2
            let vpn = (address >> (PAGE_SIZE_SHF + level * PTE_COUNT_SHF)) & PTE_COUNT_MSK;
            let entry_address = page_table + (vpn << PTE_SIZE_SHF);
            let (a, mut entry) =
                read_pte(allocator, entry_address).map_err(|_| MemoryError::AccessFault)?;
            allocator = a;
            let entry = entry.view_bits_mut::<Lsb0>();
            // STEP 3
            if !entry[pte::V] || (!entry[pte::R] && entry[pte::W]) {
                return Err(MemoryError::PageFault);
            }
            // STEP 4
            if !entry[pte::R] && !entry[pte::X] {
                // This PTE is a pointer to the next level of the page table.
                // But if we're at the last level, this is a page fault.
                if level == 0 {
                    return Err(MemoryError::PageFault);
                }
                page_table = pte::ppn(entry) << PAGE_SIZE_SHF;
                continue;
            }
            // STEP 5
            let allowed = match access_type {
                AccessType::Read => {
                    entry[pte::R]
                        || (entry[pte::X] && self.core.status.get(allocator.borrow()).mxr())
                }
                AccessType::Write => entry[pte::W],
                AccessType::Execute => entry[pte::X],
            } && {
                (user_mode == entry[pte::U])
                    || !user_mode
                        && access_type != AccessType::Execute
                        && self.core.status.get(allocator.borrow()).sum()
            };
            if !allowed {
                return Err(MemoryError::PageFault);
            }
            // STEP 6 & 8
            let mut ppn = pte::ppn(entry);
            if level != 0 {
                // STEP 6
                let mask = (1 << (level * PTE_COUNT_SHF)) - 1;
                if ppn & mask != 0 {
                    return Err(MemoryError::PageFault);
                }
                // STEP 8
                ppn |= vpn & mask;
            }
            // STEP 7
            if !entry[pte::A] || access_type == AccessType::Write && !entry[pte::D] {
                entry.set(pte::A, true);
                entry.set(pte::D, access_type == AccessType::Write);
                write_pte(allocator, entry_address, entry.load_le())
                    .map_err(|_| MemoryError::AccessFault)?;
            }
            let page_offset = address & PAGE_SIZE_MSK;
            return Ok((ppn << PAGE_SIZE_SHF) + page_offset);
        }
        // The following asserts the above loop is taken.
        const_assert!(PAGE_TABLE_LEVELS > 0);
        // The above loop can only exit through a return, hence this is unreachable.
        unreachable!()
    }

    fn read_pte(&self, allocator: &mut A, address: u32) -> Result<u32, MemoryError> {
        assert_eq!(1 << PTE_SIZE_SHF, 4);
        self.access_physical(address, 4, AccessType::Read)?;
        let mut buf = [0u8; 4];
        self.core.system_bus.read(&mut buf, allocator, address);
        Ok(u32::from_le_bytes(buf))
    }

    fn read_pte_debug(&self, allocator: &A, address: u32) -> Result<u32, MemoryError> {
        assert_eq!(1 << PTE_SIZE_SHF, 4);
        self.access_physical(address, 4, AccessType::Read)?;
        let mut buf = [0u8; 4];
        self.core
            .system_bus
            .read_debug(&mut buf, allocator, address);
        Ok(u32::from_le_bytes(buf))
    }

    fn write_pte(&self, allocator: &mut A, address: u32, value: u32) -> Result<(), MemoryError> {
        assert_eq!(1 << PTE_SIZE_SHF, 4);
        self.access_physical(address, 4, AccessType::Write)?;
        let buf = value.to_le_bytes();
        self.core.system_bus.write(allocator, address, &buf);
        Ok(())
    }
}

mod pte {
    use bitvec::{field::BitField, order::Lsb0, slice::BitSlice};

    pub const V: usize = 0;
    pub const R: usize = 1;
    pub const W: usize = 2;
    pub const X: usize = 3;
    pub const U: usize = 4;
    pub const A: usize = 6;
    pub const D: usize = 7;

    pub fn ppn(entry: &BitSlice<u32, Lsb0>) -> u32 {
        entry[10..32].load_le()
    }
}

#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum MemoryError {
    #[error("misaligned access")]
    MisalignedAccess,
    #[error("access fault")]
    AccessFault,
    #[error("page fault")]
    PageFault,
}
