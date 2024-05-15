use crate::bus::Bus;
use crate::simulator::Simulatable;
use crate::AddressRange;
use space_time::allocator::{Allocator, ArrayAccessor, ArrayAccessorMut};

/// Byte-based RAM implementation with support for misaligned memory access.
///
/// This can be categorized as *main memory* according to the types of memory resources defined by
/// the RISC-V spec.
#[derive(Debug)]
pub struct Ram<A: Allocator> {
    /// Index in the allocator where all bytes are stored.
    data: A::ArrayId<u8>,
    /// The highest byte address.
    max_address: u32,
}

impl<A: Allocator> PartialEq for Ram<A> {
    fn eq(&self, other: &Self) -> bool {
        self.data.eq(&other.data) && self.max_address == other.max_address
    }
}

impl<A: Allocator> Eq for Ram<A> {}

impl<A: Allocator> Ram<A> {
    /// Create a new zero-initialized RAM resource that can hold `size` bytes.
    ///
    /// `size` must be at least one, and at most `1 << 32` (since it must be addressable by `u32`).
    /// If `size` does not satisfy these conditions, `None` is returned and nothing is allocated.
    pub fn new(allocator: &mut A, size: usize) -> Option<Self> {
        if size == 0 || (usize::BITS > 32 && size > (1 << 32)) {
            None
        } else {
            Some(Self {
                data: allocator.insert_array(0u8, size),
                max_address: (size - 1) as u32,
            })
        }
    }

    /// Returns the size expressed in bytes. Guaranteed to be at least one.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.max_address as usize + 1
    }

    /// Returns the address range of the continuous region of bytes stored in this RAM unit.
    ///
    /// Note that `self.range().start()` will always be `0`, and `self.range().end()` always
    /// `(self.len() - 1) as u32`. This is merely a convenience function.
    pub fn range(&self) -> AddressRange {
        AddressRange::new(0, self.max_address).unwrap()
    }

    /// Force RAM back to its reset state, which is all-zeros.
    pub fn reset(&self, _allocator: &mut A) {
        // let mut data = allocator.get_array_mut(self.data).unwrap();
        // data.clear()
        todo!()
    }

    /// Reads a range of bytes from RAM into `buf`. Does not have side effects.
    ///
    /// For every address in the requested range that is within `self.range()`, the corresponding
    /// byte is written to `buf` at the offset of the address within the requested range.
    /// Elements in `buf` corresponding to addresses that do not fall within `self.range()` are left
    /// untouched.
    pub fn read(&self, buf: &mut [u8], allocator: &A, address: u32) {
        if address > self.max_address || buf.is_empty() {
            return;
        }
        const_assert!(usize::BITS >= 32);
        let size = buf.len().min((self.max_address - address) as usize + 1);
        let data = allocator.get_array(self.data).unwrap();
        match data.read(&mut buf[..size], address as usize) {
            true => (),
            false => unreachable!(),
        }
    }

    /// Writes a range of bytes from `buf` into RAM. Does not have side effects other than writing.
    ///
    /// For every address in the requested range that is within `self.range()`, the corresponding
    /// byte is written to `buf` at the offset of the address within the requested range.
    /// Elements in `buf` corresponding to addresses that do not fall within `self.range()` are
    /// ignored.
    pub fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        if address > self.max_address || buf.is_empty() {
            return;
        }
        const_assert!(usize::BITS >= 32);
        let size = buf.len().min((self.max_address - address) as usize + 1);
        let mut data = allocator.get_array_mut(self.data).unwrap();
        match data.write(address as usize, &buf[..size]) {
            true => (),
            false => unreachable!(),
        }
    }
}

impl<A: Allocator> Simulatable<A> for Ram<A> {
    fn tick(&self, allocator: &mut A) {
        let _ = allocator;
    }

    fn drop(self, allocator: &mut A) {
        allocator.remove_array(self.data).unwrap()
    }
}

impl<A: Allocator> Bus<A> for Ram<A> {
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        self.read(buf, allocator, address);
    }

    fn read_debug(&self, buf: &mut [u8], allocator: &A, address: u32) {
        self.read(buf, allocator, address);
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        self.write(allocator, address, buf);
    }
}
