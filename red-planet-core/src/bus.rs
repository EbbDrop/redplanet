//! Defines a generalization of a TileLink-like bus interface.

use crate::Allocator;
use std::fmt::Debug;
use thiserror::Error;

/// A generalization of a TileLink-like bus interface, without the hardware details.
///
/// Implementors of this trait should see it as the TileLink *slave* interface they are exposing,
/// while this interfaces serves as the TileLink *master* interface to callers of this trait.
///
/// The concept is based on what is possible using the TileLink bus interface, but with the
/// following differences:
/// - Access sizes need not be a power of two.
/// - Addresses need not be naturally aligned to the access size.
/// - There is no max data width, meaning all data is always transferred at once, rather than in
///   chunks with a size depending on the data bus width.
/// - No masking is possible, i.e. the accessed bytes are always continuous.
///
/// So, in summary, accesses can be made for any `(address, size)` pair. The addresses are 32 bits
/// wide. No limits are enforced on `size`, it can range from `0` up to `usize::MAX`.
///
/// Similar to the hardware bus protocols, the slaves implementing this interface must declare which
/// `(address, size)` pairs it supports. It decides alignment, min size, and max size requirements.
/// Additionally, it can choose how to map addresses to its internal values (e.g. multiple addresses
/// may map to the same address). However, addresses should always correspond to bytes, and values
/// should be serialized in little-endian byte order. Other than that, the slave can freely choose
/// how to treat the address space, e.g. it can choose to make it circular (such that the addresses
/// wrap around), or additionally use addresses of smaller width and return undefined values for
/// addresses outside the range, or choose to always apply a mask to the addresses.
///
/// Although the slave can set arbitrary requirements, all `(address, size)` input pairs should be
/// handled without panics! If a slave has not declared support for a certain `(address, size)`
/// combination, it may put itself in an undefined state, similar to how actual hardware could be
/// in an undefined state. However, the invariants not related to the simulated state should always
/// be preserved, i.e. only the simulated state may become undefined, not the simulating entity
/// itself. Note that an access must still remain deterministic, that is, if the same access is
/// performed twice from the same undefined state, the resulting states should be the same.
/// In particular, any number of consecutive [`read_pure`](Self::read_pure)s should always alter
/// the passed `buf` in the same way. Also note that reads cannot rely on anything in the `buf`,
/// meaning two identical reads on the same state, but with `buf`s containing different values,
/// should behave the same. In essence, the determinism must only rely on `allocator`, `address`,
/// and `buf.len()`.
///
/// The system expects little-endian byte ordering of all slave devices, and provides little-endian
/// ordering to all master devices. This means all values that are read must be serialized to bytes
/// in little-endian order. All values that are written are also sent in little-endian byte order.
///
/// The regular access methods can never fail.
/// In the future an error type for possible bus access errors may be added. These errors would be,
/// in theory, based on the errors in the TileLink protocol. TileLink specifies two possible errors:
/// data corruption, and access denied. The former cannot happen in this simulated environment. The
/// latter is only applicable if the implementation has the TL-C conformance level, i.e. it is only
/// used for caching operations. Given that caching is not (yet) implemented in this simulation,
/// this is not (yet) applicable.
pub trait Bus<A: Allocator>: Debug {
    /// Invoke a read access for `address` with size `buf.len()`, writing the result to `buf`.
    ///
    /// Note that a read with `buf.len() == 0` still performs the side effects of a read to
    /// `address`, but just cannot return any value.
    ///
    /// Values should generally be serialized in little-endian byte order.
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32);

    /// Request an effect-free read for `address` with size `buf.len()`, writing the result to
    /// `buf`.
    ///
    /// If the read is cannot be performed effect-free, a [`PureAccessError`] is returned.
    ///
    /// This differs from [`Bus::read`] in that this is guaranteed not to mutate any state (hence it
    /// only requires immutable access to the allocator), and consequently that this will not cause
    /// any side effects.
    fn read_pure(&self, buf: &mut [u8], allocator: &A, address: u32) -> PureAccessResult;

    /// Invoke a write access for `address` with size `buf.len()`, reading the data from `buf`.
    ///
    /// Note that a write with `buf.len() == 0` almost always results in undefined behavior
    /// (simulated).
    ///
    /// Values are generally deserialized in little-endian byte order.
    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]);
}

pub type PureAccessResult = Result<(), PureAccessError>;

/// Attempt to request a pure read for an `(address, size)` pair that only supports effectful
/// reads.
#[derive(Error, Debug, Clone)]
#[error("cannot read effect-free")]
pub struct PureAccessError;
