// TODO: Remove this when implementing the SpaceTime
#![allow(unused_variables, dead_code, unreachable_code)]

pub mod allocator;
pub mod errors;

use std::ops::RangeBounds;

use allocator::{Allocator, ArrayAccessor, ArrayAccessorMut};
use errors::{InvalidIdError, InvalidSnapshotIdError};

/// An [`Allocator`] with snapshotting capabilities.
#[derive(Debug, Default)]
pub struct SpaceTime {}

/// Abstract identifier of snapshots in [`SpaceTime`].
///
/// A [`SnapshotId`] created by one [`SpaceTime`] should not be used as argument to a method on a
/// different [`SpaceTime`]. However, doing so will not result in a panic, but rather communicate an
/// [`InvalidSnapshotIdError`].
///
/// [`SnapshotId`]'s can be checked for equality, but this only makes sense if they're created by
/// the same [`SpaceTime`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SnapshotId {}

impl SpaceTime {
    /// Create a new empty [`SpaceTime`].
    ///
    /// No snapshots are created, and HEAD is considered dirty ([`Self::head`] will return `None`).
    pub fn new() -> Self {
        todo!()
    }

    /// Create a new snapshot of HEAD.
    ///
    /// To check if it is sensible to create a new snapshot, `head().is_none()` can be used.
    /// It is allowed to create multiple snapshot without making changes in between.
    pub fn make_snapshot(&mut self) -> SnapshotId {
        todo!()
    }

    /// Invalidate the snapshot, potentially freeing up some space.
    ///
    /// The `snapshot_id` is invalidated when calling this, it should not be used again.
    ///
    /// For safety, snapshot ids are never reused in a single [`SpaceTime`].
    /// Calling a method with an invalidated snapshot id will cause a panic.
    pub fn drop_snapshot(&mut self, snapshot_id: SnapshotId) -> Result<(), InvalidSnapshotIdError> {
        todo!()
    }

    /// Returns an iterator over all available snapshots in no particular order.
    pub fn snapshots(&self) -> impl Iterator<Item = SnapshotId> {
        std::iter::empty() // TODO
    }

    /// Returns `true` if a snapshot with id `snapshot_id` is available.
    ///
    /// If the `snapshot_id` is created by this [`SpaceTime`], the only way this returns `false` is
    /// if [`Self::drop_snapshot`] was called in the past for this `snapshot_id`.
    ///
    /// It is allowed to pass a [`SnapshotId`] not created by this [`SpaceTime`], but it is *not*
    /// guaranteed that `false` will be returned then. That is, it is possible that a snapshot in
    /// this [`SpaceTime`] has a [`SnapshotId`] equal to the one from another [`SpaceTime`].
    pub fn has_snapshot(&self, snapshot_id: SnapshotId) -> bool {
        todo!()
    }

    /// Returns the id of the checked out snapshot, or `None` if the HEAD is dirty.
    ///
    /// The HEAD is considered dirty if changes were written since the last [`Self::checkout`] or
    /// [`Self::make_snapshot`].
    pub fn head(&self) -> Option<SnapshotId> {
        todo!()
    }

    /// Resets the state of HEAD to a snapshot.
    ///
    /// For now we assume all snapshots are stored forever if not explicitly dropped.
    pub fn checkout(&mut self, snapshot_id: SnapshotId) -> Result<(), InvalidSnapshotIdError> {
        todo!()
    }
}

impl Allocator for SpaceTime {
    type Id<T> = usize;
    type ArrayId<T> = usize;

    fn insert<T: Clone>(&mut self, object: T) -> Self::Id<T> {
        todo!()
    }

    fn insert_array<T: Copy>(&mut self, object: T, n: usize) -> Self::ArrayId<T> {
        todo!()
    }

    /// See [`Allocator::remove`].
    ///
    /// If the object is only referenced by HEAD and not by any snapshot, it will be dropped.
    fn remove<T: Clone>(&mut self, id: Self::Id<T>) -> Result<(), InvalidIdError> {
        todo!()
    }

    fn remove_array<T: Copy>(&mut self, id: Self::ArrayId<T>) -> Result<(), InvalidIdError> {
        todo!()
    }

    fn pop<T: Clone>(&mut self, id: Self::Id<T>) -> Result<T, InvalidIdError> {
        todo!()
    }

    fn get<T: Clone>(&self, id: Self::Id<T>) -> Result<&T, InvalidIdError> {
        todo!()
    }

    fn get_array<'a, T: 'a + Copy>(
        &'a self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessor<'a, T>, InvalidIdError> {
        todo!() as Result<STArrayAccessor, _>
    }

    fn get_mut<T: Clone>(&mut self, id: Self::Id<T>) -> Result<&mut T, InvalidIdError> {
        todo!()
    }

    fn get_array_mut<'a, T: 'a + Copy>(
        &'a mut self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessorMut<'a, T>, InvalidIdError> {
        todo!() as Result<STArrayAccessorMut, _>
    }
}

#[derive(Debug)]
struct STArrayAccessor<'a> {
    st: &'a SpaceTime,
}

impl<'a, T: 'a + Copy> ArrayAccessor<'a, T> for STArrayAccessor<'a> {
    fn len(&self) -> usize {
        todo!()
    }

    fn get(&self, index: usize) -> Option<T> {
        todo!()
    }

    fn get_ref(&self, index: usize) -> Option<&'a T> {
        todo!()
    }

    fn read(&self, buf: &mut [T], index: usize) -> bool {
        todo!()
    }

    fn iter_range<R>(&self, index_range: R) -> Option<impl IntoIterator<Item = &'a T> + 'a>
    where
        R: RangeBounds<usize>,
    {
        todo!() as Option<&'a [T]>
    }
}

#[derive(Debug)]
struct STArrayAccessorMut<'a> {
    st: &'a mut SpaceTime,
}

impl<'a, T: 'a + Copy> ArrayAccessor<'a, T> for STArrayAccessorMut<'a> {
    fn len(&self) -> usize {
        todo!()
    }

    fn get(&self, index: usize) -> Option<T> {
        todo!()
    }

    fn get_ref(&self, index: usize) -> Option<&'a T> {
        todo!()
    }

    fn read(&self, buf: &mut [T], index: usize) -> bool {
        todo!()
    }

    fn iter_range<R>(&self, index_range: R) -> Option<impl IntoIterator<Item = &'a T> + 'a>
    where
        R: RangeBounds<usize>,
    {
        todo!() as Option<&'a [T]>
    }
}

impl<'a, T: 'a + Copy> ArrayAccessorMut<'a, T> for STArrayAccessorMut<'a> {
    fn get_mut(&self, index: usize) -> Option<&'a mut T> {
        todo!()
    }

    fn set(&self, index: usize, value: T) -> bool {
        todo!()
    }

    fn write(&self, index: usize, buf: &[T]) -> bool {
        todo!()
    }

    fn iter_range_mut<R>(&self, index_range: R) -> Option<impl IntoIterator<Item = &'a mut T> + 'a>
    where
        R: RangeBounds<usize>,
    {
        todo!() as Option<&'a mut [T]>
    }
}
