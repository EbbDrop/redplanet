use crate::{address_range, AddressRange};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use thiserror::Error;

/// Generic map of 32-bit address ranges to values of type `T`.
///
/// The ranges cannot overlap.
#[derive(Debug)]
pub struct AddressMap<T> {
    ordered_ranges: Vec<(AddressRange, T)>,
}

impl<T> Default for AddressMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AddressMap<T> {
    /// Create new empty map.
    pub fn new() -> Self {
        Self {
            ordered_ranges: Vec::new(),
        }
    }

    /// Returns the address range that contains `address`.
    ///
    /// Note that even if `address` maps to a vacant region, that region's range will be returned.
    pub fn range(&self, address: u32) -> AddressRange {
        self.range_value(address).0
    }

    /// Returns the value that the address range containing `address` maps to, or `None` if that
    /// address range is vacant.
    pub fn value(&self, address: u32) -> Option<&T> {
        self.range_value(address).1
    }

    /// Returns the address range that contains `address`, and the value that it maps to.
    ///
    /// The second item will be `None` if `address` is in a vacant region.
    pub fn range_value(&self, address: u32) -> (AddressRange, Option<&T>) {
        match self.ordered_ranges.binary_search_by(|(range, _)| {
            if address < range.start() {
                Ordering::Less
            } else if address <= range.end() {
                Ordering::Equal
            } else {
                Ordering::Greater
            }
        }) {
            Ok(index) => {
                let (range, value) = &self.ordered_ranges[index];
                (*range, Some(value))
            }
            Err(index) => {
                let start = index
                    .checked_sub(1)
                    .and_then(|i| self.ordered_ranges.get(i))
                    // The addition is guaranteed not to overflow, since that would mean
                    // `range.end() == u32::MAX`, which is impossible since `address > range.end()`
                    // according to the binary search.
                    .map(|(range, _)| range.end() + 1)
                    .unwrap_or(0);
                let end = self
                    .ordered_ranges
                    .get(index)
                    // The subtraction is guaranteed not to underflow, since that would mean
                    // `range.start() == 0`, which is impossible since `address < range.start()`
                    // according to the binary search.
                    .map(|(range, _)| range.start() - 1)
                    .unwrap_or(u32::MAX);
                (address_range![start, end], None)
            }
        }
    }
}

impl<T> TryFrom<Vec<(AddressRange, T)>> for AddressMap<T> {
    type Error = AddressMapError;

    fn try_from(mut value: Vec<(AddressRange, T)>) -> Result<Self, Self::Error> {
        value.sort_by_key(|(range, _)| range.start());

        let mut iter = value.iter();
        if let Some((mut prev_range, _)) = iter.next() {
            for &(range, _) in iter {
                if range.start() <= prev_range.end() {
                    return Err(AddressMapError::OverlappingAddressRanges);
                }
                prev_range = range;
            }
        }

        Ok(Self {
            ordered_ranges: value,
        })
    }
}

#[derive(Error, Debug)]
pub enum AddressMapError {
    /// Attempt to add an address range that overlaps with a previously added address range.
    #[error("address range overlaps with previously added address range")]
    OverlappingAddressRanges,
}

#[macro_export]
macro_rules! addr_map {
    ($([$start:expr, $end:expr] => $value:expr,)*) => {
        $crate::address_map::AddressMap::try_from(vec![
            $(($crate::address_range![$start, $end], $value)),*
        ]).unwrap()
    };
}

/// An [`AddressMap`] that can also map values to addresses.
///
/// Every value can only be associated with one address range (i.e. injection).
///
/// The ranges cannot overlap.
#[derive(Debug)]
pub struct TwoWayAddressMap<T: Clone + Eq + Hash> {
    address_map: AddressMap<T>,
    inverse_map: HashMap<T, AddressRange>,
}

impl<T: Clone + Eq + Hash> Default for TwoWayAddressMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Eq + Hash> TwoWayAddressMap<T> {
    /// Create new empty map.
    pub fn new() -> Self {
        Self {
            address_map: AddressMap::new(),
            inverse_map: HashMap::new(),
        }
    }

    /// Returns the address range that maps to `value`, or `None` if there is no such address range.
    pub fn range_for(&self, value: &T) -> Option<AddressRange> {
        self.inverse_map.get(value).copied()
    }

    /// Returns the address range that contains `address`.
    ///
    /// Note that even if `address` maps to a vacant region, that region's range will be returned.
    pub fn range(&self, address: u32) -> AddressRange {
        self.address_map.range(address)
    }

    /// Returns the value that the address range containing `address` maps to, or `None` if that
    /// address range is vacant.
    pub fn value(&self, address: u32) -> Option<&T> {
        self.address_map.value(address)
    }

    /// Returns the address range that contains `address`, and the value that it maps to.
    ///
    /// The second item will be `None` if `address` is in a vacant region.
    pub fn range_value(&self, address: u32) -> (AddressRange, Option<&T>) {
        self.address_map.range_value(address)
    }
}

impl<T: Clone + Eq + Hash> TryFrom<Vec<(AddressRange, T)>> for TwoWayAddressMap<T> {
    type Error = TwoWayAddressMapError;

    fn try_from(value: Vec<(AddressRange, T)>) -> Result<Self, Self::Error> {
        let address_map = AddressMap::try_from(value)?;

        let mut inverse_map = HashMap::with_capacity(address_map.ordered_ranges.len());

        for (range, value) in &address_map.ordered_ranges {
            let old = inverse_map.insert(value.clone(), *range);
            if old.is_some() {
                return Err(TwoWayAddressMapError::DuplicateValues);
            }
        }

        Ok(Self {
            address_map,
            inverse_map,
        })
    }
}

#[derive(Error, Debug)]
pub enum TwoWayAddressMapError {
    /// Attempt to add an address range that overlaps with a previously added address range.
    #[error("address range overlaps with previously added address range")]
    OverlappingAddressRanges,
    /// Attempt to add an (address range -> value) pair when a pair with the same value already
    /// exists.
    #[error("value equals previously added value")]
    DuplicateValues,
}

impl From<AddressMapError> for TwoWayAddressMapError {
    fn from(value: AddressMapError) -> Self {
        match value {
            AddressMapError::OverlappingAddressRanges => Self::OverlappingAddressRanges,
        }
    }
}

#[macro_export]
macro_rules! two_way_addr_map {
    ($([$start:expr, $end:expr] <=> $value:expr,)*) => {
        $crate::address_map::TwoWayAddressMap::try_from(vec![
            $(($crate::address_range![$start, $end], $value)),*
        ]).unwrap()
    };
}
