use crate::bus::{Bus, PureAccessResult};
use crate::{AddressRange, Allocator};
use any_cmp::AnyEq;
use rangemap::RangeInclusiveMap;
use std::fmt::Debug;
use std::ops::Deref;
use std::rc::Rc;
use thiserror::Error;

/// Extension of the standard *slave* interface ([`Bus`]) that is needed for an agent to attach to a
/// [`SystemBus`].
pub trait Slave<A: Allocator>: Bus<A> + AnyEq {}

impl<A: Allocator> PartialEq for dyn Slave<A> {
    fn eq(&self, other: &Self) -> bool {
        self.any_eq(other.as_any_partial_eq_ref())
    }
}

impl<A: Allocator> Eq for dyn Slave<A> {}

/// Abstraction of a (TileLink) crossbar providing a single *master* interface for the entire 32-bit
/// physical address space, and delegating requests to the appropriate agent's *slave* interface
/// depending on a configurable address mapping.
///
/// The address mapping is in the form of a set of memory regions. The RISC-V specification defines
/// three types of memory regions:
/// - *vacant*: address range mapped to nothing
/// - *main memory*: address range mapped to main memory
/// - *I/O regions*: address range mapped to I/O devices (anything that's not main memory)
///
/// Reads and writes of I/O devices may have visible side effects, but accesses to main memory
/// cannot. This crossbar does not differentiate between main memory and I/O regions.
///
/// The configurable mapping must satisfy the following conditions:
/// - no two memory regions may overlap
/// - all memory regions must be contained within the 32-bit address space
///
/// Note that vacant memory regions (i.e. unmapped address ranges) are allowed, but accessing them
/// will do nothing.
///
/// Accesses are always in the form of `(address, size)` pairs. The access request is forwarded to
/// the *slave* interface that `address` maps to, if and only if the entire address range
/// `address..(address+size)` is contained within the memory region that `address` is in. Otherwise,
/// the access is not forwarded and will do nothing.
#[derive(Debug, Default)]
pub struct SystemBus<A: Allocator> {
    /// Map of physical address range to `(resource_index, base_address)` pair, where
    /// - `resource_index` is the index in `resources` of the resource to which the range is mapped
    /// - `base_address` is the base address of the resource for this mapped region, that is, the
    ///   offset of each address in the physical address range relative to the start of that range
    ///   is added to this `base_address` to become the address that is passed to the resource
    regions: RangeInclusiveMap<u32, (usize, u32)>,
    slaves: Vec<Rc<dyn Slave<A>>>,
}

impl<A: Allocator> SystemBus<A> {
    pub fn new() -> Self {
        Self {
            regions: RangeInclusiveMap::new(),
            slaves: Vec::new(),
        }
    }

    /// Chainable version of [`Self::attach_resource`].
    pub fn with_resource(
        mut self,
        slave: Rc<dyn Slave<A>>,
        mappings: impl IntoIterator<Item = (AddressRange, AddressRange)>,
    ) -> Result<Self, ResourceMappingError> {
        self.attach_resource(slave, mappings).map(|()| self)
    }

    /// Attaches the `slave` interface.
    ///
    /// Returns `Err(())` if any region in `mappings` overlaps with any region of a previously added
    /// resource or with another region in `mappings`.
    ///
    /// Returns `Err(())` if the there is a `(range, base_address)` pair for which the following
    /// assumption does *not* hold: `range.end() - range.start() <= u32::MAX - base_address`.
    /// This is so every element in the range is mapped to a resource address that fits in `u32`.
    pub fn attach_resource(
        &mut self,
        slave: Rc<dyn Slave<A>>,
        mappings: impl IntoIterator<Item = (AddressRange, AddressRange)>,
    ) -> Result<(), ResourceMappingError> {
        let index = self.slaves.len();
        self.slaves.push(slave);
        for (source_range, target_range) in mappings {
            if self.regions.overlaps(&source_range.into()) {
                return Err(ResourceMappingError::OverlappingSourceRegions);
            }
            if source_range.cmp_size(target_range).is_ne() {
                return Err(ResourceMappingError::IncompatibleTargetRegion);
            }
            self.regions
                .insert(source_range.into(), (index, target_range.start()));
        }
        Ok(())
    }

    /// Detaches all resources equal to `resource` from the system bus.
    pub fn detach_resource(&mut self, resource: &dyn Slave<A>) {
        self.slaves.retain(|r| r.deref() != resource);
    }

    fn access(&self, address: u32, size: usize) -> Result<(&dyn Slave<A>, u32), AccessError> {
        self.regions
            .get_key_value(&address)
            .ok_or(AccessError::UnmappedAddress)
            .and_then(|(range, &(resource_index, base_address))| {
                const_assert!(usize::BITS >= 32);
                if size == 0 || size - 1 < (range.end() - address) as usize {
                    let mapped_address = base_address + (address - range.start());
                    Ok((&*self.slaves[resource_index], mapped_address))
                } else {
                    Err(AccessError::RangeExceedsRegion)
                }
            })
    }
}

impl<A: Allocator> Bus<A> for SystemBus<A> {
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        #[allow(clippy::single_match)]
        match self.access(address, buf.len()) {
            Ok((resource, mapped_address)) => resource.read(buf, allocator, mapped_address),
            // If no region is being accessed, or the access is not valid, nothing happens.
            Err(_) => (),
        }
    }

    fn read_pure(&self, buf: &mut [u8], allocator: &A, address: u32) -> PureAccessResult {
        match self.access(address, buf.len()) {
            Ok((resource, mapped_address)) => resource.read_pure(buf, allocator, mapped_address),
            // If no region is being accessed, or the access is not valid, nothing happens.
            Err(_) => Ok(()),
        }
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        #[allow(clippy::single_match)]
        match self.access(address, buf.len()) {
            Ok((resource, mapped_address)) => resource.write(allocator, mapped_address, buf),
            // If no region is being accessed, or the access is not valid, nothing happens.
            Err(_) => (),
        }
    }
}

#[derive(Error, Debug)]
pub enum ResourceMappingError {
    /// The range of physical addresses mapped to the resource overlaps with another range for the
    /// same resource, or with a range from an already attached resource.
    #[error("memory region mapping overlaps with previously mapped memory region")]
    OverlappingSourceRegions,
    /// The physical address range and the range of resource addresses differ in size
    #[error("source and target memory region differ in size")]
    IncompatibleTargetRegion,
}

#[derive(Error, Debug, Clone)]
pub enum AccessError {
    /// Attempt to access an address that falls within a vacant memory region.
    #[error("address maps to vacant memory region")]
    UnmappedAddress,
    /// Attempt to access an address range that crosses memory region boundaries.
    #[error("address range exceeds memory region boundary")]
    RangeExceedsRegion,
}
