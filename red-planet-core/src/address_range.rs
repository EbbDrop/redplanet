use std::cmp::Ordering;
use std::collections::Bound;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::ops::{Range, RangeBounds, RangeInclusive};
use std::slice::SliceIndex;
use thiserror::Error;

/// A non-empty range in a 32-bit address space bounded inclusively below and above.
///
/// Enforces the invariant that `self.start() <= self.end()`.
///
/// Note that this is indifferent as to what is addressed, this can be bytes, words, or anything
/// else.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct AddressRange {
    start: u32,
    end: u32,
}

impl Default for AddressRange {
    fn default() -> Self {
        Self::full()
    }
}

impl Display for AddressRange {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[{:#x}, {:#x}]", self.start, self.end)
    }
}

impl AddressRange {
    pub fn new(start: u32, end: u32) -> Result<Self, InvalidBoundsError> {
        (start <= end)
            .then_some(Self { start, end })
            .ok_or(InvalidBoundsError { start, end })
    }

    /// Create a new address range covering all possible 32-bit addresses.
    pub fn full() -> Self {
        Self {
            start: 0,
            end: u32::MAX,
        }
    }

    pub fn start(self) -> u32 {
        self.start
    }

    pub fn end(self) -> u32 {
        self.end
    }

    pub fn with_start(self, start: u32) -> Result<Self, InvalidBoundsError> {
        (start <= self.end)
            .then_some(Self {
                start,
                end: self.end,
            })
            .ok_or(InvalidBoundsError {
                start,
                end: self.end,
            })
    }

    pub fn with_end(self, end: u32) -> Result<Self, InvalidBoundsError> {
        (self.start <= end)
            .then_some(Self {
                start: self.start,
                end,
            })
            .ok_or(InvalidBoundsError {
                start: self.start,
                end,
            })
    }

    /// Set a new start address, returning the old start.
    pub fn set_start(&mut self, start: u32) -> Result<u32, InvalidBoundsError> {
        if start <= self.end {
            let old_start = self.start;
            self.start = start;
            Ok(old_start)
        } else {
            Err(InvalidBoundsError {
                start,
                end: self.end,
            })
        }
    }

    /// Set a new end address, returning the old end.
    pub fn set_end(&mut self, end: u32) -> Result<u32, InvalidBoundsError> {
        if self.start <= end {
            let old_end = self.end;
            self.end = end;
            Ok(old_end)
        } else {
            Err(InvalidBoundsError {
                start: self.start,
                end,
            })
        }
    }

    /// Check if an address is contained within this address range.
    pub fn contains(self, address: u32) -> bool {
        self.start <= address && address <= self.end
    }

    /// Returns `self.end() - self.start()`, which is the size minus 1.
    ///
    /// This value is always within the range `0..=u32::MAX`.
    pub fn delta(self) -> u32 {
        self.end - self.start
    }

    /// Returns the size of this address range if it is representable by a `usize`, or `None`
    /// otherwise.
    pub fn size(self) -> Option<usize> {
        usize::try_from(self.delta())
            .ok()
            .and_then(|n| n.checked_add(1))
    }

    /// Compare the size of this address range to another.
    pub fn cmp_size(self, other: Self) -> Ordering {
        // Since this is a comparison, it doesn't matter that we're comparing "size - 1" of both
        self.delta().cmp(&other.delta())
    }

    // TODO: Maybe make the slice type generic?
    //       Or is it better to leave it to only byte slices as a safeguard?
    pub fn to_index(self) -> impl SliceIndex<[u8], Output = [u8]> {
        const_assert!(usize::BITS >= 32);
        (self.start as usize)..=(self.end as usize)
    }
}

impl TryFrom<RangeInclusive<u32>> for AddressRange {
    type Error = InvalidBoundsError;

    fn try_from(value: RangeInclusive<u32>) -> Result<Self, Self::Error> {
        Self::new(*value.start(), *value.end())
    }
}

impl TryFrom<Range<u32>> for AddressRange {
    type Error = InvalidBoundsError;

    fn try_from(value: Range<u32>) -> Result<Self, Self::Error> {
        match value.end.checked_sub(1) {
            Some(end) => Self::new(value.start, end),
            None => Err(InvalidBoundsError {
                start: value.start,
                end: value.end,
            }),
        }
    }
}

impl From<AddressRange> for RangeInclusive<u32> {
    fn from(value: AddressRange) -> Self {
        value.start..=value.end
    }
}

impl TryFrom<AddressRange> for Range<u32> {
    type Error = UnrepresentableBoundsError;

    fn try_from(value: AddressRange) -> Result<Self, Self::Error> {
        match value.end.checked_add(1) {
            Some(end) => Ok(value.start..end),
            None => Err(UnrepresentableBoundsError(value)),
        }
    }
}

impl RangeBounds<u32> for AddressRange {
    fn start_bound(&self) -> Bound<&u32> {
        Bound::Included(&self.start)
    }

    fn end_bound(&self) -> Bound<&u32> {
        Bound::Included(&self.end)
    }
}

#[derive(Error, Debug, Clone)]
#[error("bounds [{start:#x}, {end:#x}] do not form a valid 32-bit address range")]
pub struct InvalidBoundsError {
    start: u32,
    end: u32,
}

#[derive(Error, Debug, Clone)]
#[error("upper bound of address range {0} not representable by 32-bit exclusive range")]
pub struct UnrepresentableBoundsError(AddressRange);

#[macro_export]
macro_rules! address_range {
    ($start:expr, $end:expr) => {
        $crate::address_range::AddressRange::new($start, $end).unwrap()
    };
}
