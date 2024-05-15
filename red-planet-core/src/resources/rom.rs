use crate::bus::Bus;
use crate::simulator::Simulatable;
use crate::AddressRange;
use space_time::allocator::{Allocator, ArrayAccessor, ArrayAccessorMut};

/// Byte-based ROM implementation with support for misaligned memory access.
///
/// This can be categorized as *main memory* according to the types of memory resources defined by
/// the RISC-V spec.
#[derive(Debug)]
pub struct Rom<A: Allocator> {
    /// Index in the allocator where all data-holding bytes are stored.
    data: A::ArrayId<u8>,
    /// The length of the array stored at `data`.
    data_len: usize,
    /// The highest byte address that is mapped.
    max_address: u32,
}

impl<A: Allocator> PartialEq for Rom<A> {
    fn eq(&self, other: &Self) -> bool {
        self.data.eq(&other.data)
            && self.data_len == other.data_len // This should always be true if the previous is true
            && self.max_address == other.max_address
    }
}

impl<A: Allocator> Eq for Rom<A> {}

impl<A: Allocator> Rom<A> {
    /// Create a new ROM device that holds `size` bytes, of which the first bytes are initialized
    /// with `buf`.
    ///
    /// Only up to `size` bytes are read from `buf`, others are ignored.
    ///
    /// `size` must be at least one, and at most `1 << 32` (since it must be addressable by `u32`).
    /// If `size` does not satisfy these conditions, `None` is returned and nothing is allocated.
    pub fn new(allocator: &mut A, size: usize, buf: &[u8]) -> Option<Self> {
        if size == 0 || (usize::BITS > 32 && size > (1 << 32)) {
            None
        } else {
            let data = allocator.insert_array(0u8, size);
            let data_len = buf.len().min(size);
            if data_len > 0 {
                let mut array = allocator.get_array_mut(data).unwrap();
                match array.write(0, &buf[..data_len]) {
                    true => {}
                    false => unreachable!(),
                }
            }
            Some(Self {
                data,
                data_len,
                max_address: (size - 1) as u32,
            })
        }
    }

    /// Returns the size expressed in bytes. Guaranteed to be at least one.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.max_address as usize + 1
    }

    /// Returns the address range of the continuous region of bytes stored in this ROM unit
    ///
    /// Note that `self.range().start()` will always be `0`, and `self.range().end()` always
    /// `(self.len() - 1) as u32`. This is merely a convenience function.
    pub fn range(&self) -> AddressRange {
        AddressRange::new(0, self.max_address).unwrap()
    }

    /// Reads a range of bytes from ROM into `buf`. Does not have side effects.
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
        match data.read(&mut buf[..size.min(self.data_len)], address as usize) {
            true => (),
            false => unreachable!(),
        }
        if size > self.data_len {
            buf[self.data_len..size].fill(0);
        }
    }
}

impl<A: Allocator> Simulatable<A> for Rom<A> {
    fn tick(&self, allocator: &mut A) {
        let _ = allocator;
    }

    fn drop(self, allocator: &mut A) {
        allocator.remove_array(self.data).unwrap()
    }
}

impl<A: Allocator> Bus<A> for Rom<A> {
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        self.read(buf, allocator, address);
    }

    fn read_debug(&self, buf: &mut [u8], allocator: &A, address: u32) {
        self.read(buf, allocator, address);
    }

    /// See [`Bus::write`].
    ///
    /// Writes are always ignored.
    fn write(&self, _allocator: &mut A, _address: u32, _buf: &[u8]) {}
}
