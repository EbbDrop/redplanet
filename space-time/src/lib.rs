// TODO: Remove this when implementing the SpaceTime
#![allow(unused_variables, dead_code)]

pub mod change;
pub mod errors;
pub mod region;

use change::Change;
use errors::{GoToStepIdError, WriteError};
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
pub struct StepId {}

/// An array of `u32`s that can be restored to earlier state.
#[derive(Debug, Clone)]
pub struct SpaceTime {}

impl SpaceTime {
    /// Get a reference to a certain region to allow reads to that region.
    pub fn get_region(&self, region: RegionHandle) -> Region<'_> {
        Region { space_time: self }
    }

    /// Get a reference to a certain region to allow reads and writes to that region.
    ///
    /// # Panic
    /// Will panic when the current step is in the past and future data exits.
    /// Call [`remove_future`] before calling this method to change data while
    /// staring from the past.
    ///
    /// [`remove_future`]: Self::remove_future
    pub fn get_region_mut(&mut self, region: RegionHandle) -> RegionMut<'_> {
        RegionMut { space_time: self }
    }

    /// Get the id of the current step.
    ///
    /// The returned [`StepId`] can be used to go back to the state the
    /// [`SpaceTime`] is in right now.
    pub fn step_id(&mut self) -> StepId {
        todo!()
    }

    /// All the changes since the current given step.
    pub fn changes_since_step(&self, id: StepId) -> impl Iterator<Item = Change> {
        None::<Change>.into_iter()
    }

    /// Returns `Ok(())` if it is possible to jump to the given step id.
    /// Otherwise returns the error [`go_to_step_id`] would give.
    ///
    /// [`go_to_step_id`]: Self::go_to_step_id
    pub fn step_id_available(&self, id: StepId) -> Result<(), GoToStepIdError> {
        todo!()
    }

    /// Sets the step id to the given [`StepId`].
    ///
    /// If this function returns an `Err`, the current step is not changed.
    /// Use [`step_id_available`] to see if it would be possible to use this
    /// method with a certain [`StepId`].
    ///
    /// [`step_id_available`]: Self::step_id_available
    pub fn go_to_step_id(&mut self, id: StepId) -> Result<(), GoToStepIdError> {
        todo!()
    }

    /// Removes all data for all steps in the future. Allowing a new future
    /// to be created.
    ///
    /// This will invalidate all the [`StepId`]'s that where created after
    /// the current one. But [`StepId`]'s created after calling this function
    /// could have the same value.
    pub fn remove_future(&mut self) {
        todo!()
    }
}
