use crate::region::RegionHandle;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Change {
    /// The region the change happened in.
    pub region: RegionHandle,
    /// The position within the region.
    pub position: u32,
    pub change: ChangeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChangeType {
    Write {
        /// The old value before the change at the location.
        old: u32,
        /// The new value after the change at the location.
        new: u32,
    },
    Read {
        /// The value in the location at the time of the read.
        value_read: u32,
    },
}
