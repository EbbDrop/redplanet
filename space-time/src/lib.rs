// TODO: Remove this when implementing the SpaceTime
#![allow(unused_variables, dead_code)]

pub mod errors;
pub mod region;

use errors::WriteError;
use region::{Region, RegionHandle, RegionMut};

#[derive(Default, Debug)]
pub struct SpaceTimeBuilder {}

impl SpaceTimeBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub fn add_region(&mut self, size: u32) -> RegionHandle {
        todo!()
    }

    /// Sets up some data to have directly after building the [`SpaceTime`].
    ///
    /// The data in `data` will be saved in the given region at the given
    /// index. With the first word of `data` stored at `index`.
    ///
    /// The created [`SpaceTime`] will *not* store any history related to this write.
    /// And the first step will always contain this data.
    ///
    /// A [`WriteError`] will be returned when trying to store data outside of the region.
    pub fn add_initial_data(
        &mut self,
        region: RegionHandle,
        index: u32,
        data: &[u32],
    ) -> Result<(), WriteError> {
        todo!()
    }

    pub fn build(self) -> SpaceTime {
        todo!()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId {}

/// An array of `u32`s that can be restored to earlier state.
#[derive(Debug, Clone)]
pub struct SpaceTime {}

impl SpaceTime {
    /// Get a reference to a certain region to allow reads to that region.
    pub fn get_region(&self, region: RegionHandle) -> Region<'_> {
        Region { space_time: self }
    }

    /// Get a reference to a certain region to allow reads and writes to that region.
    pub fn get_region_mut(&mut self, region: RegionHandle) -> RegionMut<'_> {
        RegionMut { space_time: self }
    }

    /// Create a new snapshot
    pub fn make_snapshot(&mut self) -> SnapshotId {
        todo!()
    }

    /// Returns `true` if it is possible to jump to the given snapshot.
    ///
    /// The only case this can return `false` at the moment if the data
    /// for the `StepId` has be deleted to save on memory.
    pub fn snapshot_available(&self, id: SnapshotId) -> bool {
        todo!()
    }

    /// Go to the snapshot with the given [`SnapshotId`].
    ///
    /// If this function returns `false`, the snapshot is not changed.
    ///
    /// The only case this can return `false` at the moment if the data
    /// for the `SnapshotId` has be deleted to save on memory.
    ///
    /// Use [`snapshot_available`] to see if it would be possible to use this
    /// method with a certain [`SnapshotId`] without actively changing the
    /// snapshot.
    ///
    /// [`snapshot_available`]: Self::snapshot_available
    pub fn go_to_snapshot(&mut self, id: SnapshotId) -> bool {
        todo!()
    }

    /// Removes all data for all snapshots and non deterministic writes
    /// in the future. Allowing a new future to be created.
    ///
    /// This will invalidate all the [`SnapshotId`]'s that where created after
    /// the current one. But [`SnapshotId`]'s created after calling this function
    /// could have the same value.
    pub fn remove_future(&mut self) {
        todo!()
    }
}
