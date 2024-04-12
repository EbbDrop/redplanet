//! Optimization ideas:
//!
//! - make SpaceTime::tables a special TypeMap that is able to store a spacial Table that has the
//!   any internally to remove a level of indirection from the Box

pub mod allocator;
mod array_storage;
pub mod errors;
mod ids;
mod snapshot;
mod table;
mod typemap;

use std::ops::RangeBounds;

use allocator::{Allocator, ArrayAccessor, ArrayAccessorMut};
use array_storage::{ArrayStorage, Instance};
use errors::{InvalidIdError, InvalidSnapshotIdError};
use generational_arena::{Arena, Index};
use ids::SpaceTimeId;
use snapshot::{Snapshot, TypedInstance, TypedTablePtr};
use table::TableTrait;
use typemap::{ArrayStorageTypeMap, TableTypeMap};

#[derive(Debug, Default)]
pub struct SpaceTimeStorage {
    tables: TableTypeMap,
    array_storage: ArrayStorageTypeMap,
}

impl SpaceTimeStorage {
    fn clone_snapshot(&mut self, snapshot: &Snapshot) -> Snapshot {
        let table_ptrs = snapshot
            .iter_table_ptrs()
            .map(|TypedTablePtr { table_ptr, type_id }| {
                let table = self.tables.get_with_id_mut(*type_id).expect(
                    "type should be in the tables if they are in a snapshot made by this SpaceTime",
                );

                let table_ptr = table.clone_table_ptr(table_ptr);

                TypedTablePtr {
                    table_ptr,
                    type_id: *type_id,
                }
            });
        let instances = snapshot
            .iter_instances()
            .map(|TypedInstance { instance, type_id }| {
                let array_storage = self.array_storage.get_with_id_mut(*type_id).expect(
                    "type should be in the array_storage if they are in a snapshot made by this SpaceTime",
                );

                let instance = array_storage.clone_instance(instance);

                TypedInstance {
                    instance,
                    type_id: *type_id,
                }
            });

        Snapshot::from_iterators(table_ptrs, instances)
    }

    fn delete_snapshot(&mut self, snapshot: Snapshot) {
        let (table_ptrs, instances) = snapshot.into_iterators();

        for TypedTablePtr { table_ptr, type_id } in table_ptrs {
            let table = self.tables.get_with_id_mut(type_id).expect(
                "type should be in the tables if they are in a snapshot made by this SpaceTime",
            );

            table.drop_table_ptr(table_ptr);
        }

        for TypedInstance { instance, type_id } in instances {
            let array_storage = self.array_storage.get_with_id_mut(type_id).expect(
                "type should be in the array_storage if they are in a snapshot made by this SpaceTime",
            );

            array_storage.drop_instance(instance);
        }
    }
}

#[derive(Debug, Default)]
struct SpaceTimeSnapshots {
    head: Head,
    snapshots: Arena<Snapshot>,
}

#[derive(Debug)]
enum Head {
    Dirty(Snapshot),
    Checkout(SnapshotId),
}

impl SpaceTimeSnapshots {
    fn head_id(&self) -> Option<SnapshotId> {
        match &self.head {
            Head::Checkout(snapshot_id) => Some(*snapshot_id),
            _ => None,
        }
    }

    fn get_head(&self) -> &Snapshot {
        match &self.head {
            Head::Dirty(s) => s,
            Head::Checkout(snapshot_id) => self
                .snapshots
                .get(snapshot_id.index)
                .expect("head should point to a vaild snapshot"),
        }
    }

    fn get_head_mut<'a>(&'a mut self, storage: &mut SpaceTimeStorage) -> &'a mut Snapshot {
        // This code is like this to work around some borrow checker limitations.
        let Head::Checkout(snapshot_id) = &self.head else {
            if let Head::Dirty(s) = &mut self.head {
                return s;
            }
            unreachable!()
        };

        let ref_head = self
            .snapshots
            .get(snapshot_id.index)
            .expect("head should point to a vaild snapshot");

        let new_snapshot = storage.clone_snapshot(ref_head);

        self.head = Head::Dirty(new_snapshot);
        let Head::Dirty(new_snapshot) = &mut self.head else {
            unreachable!()
        };
        new_snapshot
    }

    pub fn make_snapshot(&mut self, storage: &mut SpaceTimeStorage) -> SnapshotId {
        let head = self.get_head();
        let snapshot = storage.clone_snapshot(head);

        let index = self.snapshots.insert(snapshot);

        let id = SnapshotId { index };
        self.head = Head::Checkout(id);

        id
    }

    pub fn drop_snapshot(
        &mut self,
        snapshot_id: SnapshotId,
        storage: &mut SpaceTimeStorage,
    ) -> Result<(), InvalidSnapshotIdError> {
        let snapshot = self
            .snapshots
            .remove(snapshot_id.index)
            .ok_or(InvalidSnapshotIdError)?;

        match &self.head {
            Head::Checkout(index) if index == &snapshot_id => {
                self.head = Head::Dirty(snapshot);
            }
            _ => {
                storage.delete_snapshot(snapshot);
            }
        }
        Ok(())
    }

    pub fn snapshots(&self) -> impl Iterator<Item = SnapshotId> + '_ {
        self.snapshots
            .iter()
            .map(|(index, _snapshot)| SnapshotId { index })
    }

    pub fn contains(&self, snapshot_id: SnapshotId) -> bool {
        self.snapshots.contains(snapshot_id.index)
    }

    pub fn checkout(
        &mut self,
        snapshot_id: SnapshotId,
        storage: &mut SpaceTimeStorage,
    ) -> Result<(), InvalidSnapshotIdError> {
        if !self.snapshots.contains(snapshot_id.index) {
            return Err(InvalidSnapshotIdError);
        }

        match &self.head {
            Head::Dirty(_) => {
                let old_head = std::mem::replace(&mut self.head, Head::Checkout(snapshot_id));
                let Head::Dirty(old_head) = old_head else {
                    unreachable!()
                };

                storage.delete_snapshot(old_head);
            }
            Head::Checkout(_) => self.head = Head::Checkout(snapshot_id),
        }

        Ok(())
    }
}

/// An [`Allocator`] with snapshotting capabilities.
#[derive(Debug, Default)]
pub struct SpaceTime {
    storage: SpaceTimeStorage,
    snapshots: SpaceTimeSnapshots,
}

impl Default for Head {
    fn default() -> Self {
        Self::Dirty(Snapshot::default())
    }
}

/// Abstract identifier of snapshots in [`SpaceTime`].
///
/// A [`SnapshotId`] created by one [`SpaceTime`] should not be used as argument to a method on a
/// different [`SpaceTime`]. However, doing so will not result in a panic, but rather communicate an
/// [`InvalidSnapshotIdError`].
///
/// [`SnapshotId`]'s can be checked for equality, but this only makes sense if they're created by
/// the same [`SpaceTime`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SnapshotId {
    index: Index,
}

impl SpaceTime {
    /// Create a new empty [`SpaceTime`].
    ///
    /// No snapshots are created, and HEAD is considered dirty ([`Self::head`] will return `None`).
    pub fn new() -> Self {
        SpaceTime::default()
    }

    /// Create a new snapshot of HEAD. And check it out.
    ///
    /// To check if it is sensible to create a new snapshot, `head().is_none()` can be used.
    /// It is allowed to create multiple snapshot without making changes in between.
    pub fn make_snapshot(&mut self) -> SnapshotId {
        self.snapshots.make_snapshot(&mut self.storage)
    }

    /// Invalidate the snapshot, potentially freeing up some space.
    ///
    /// The `snapshot_id` is invalidated when calling this, it should not be used again.
    ///
    /// For safety, snapshot ids are never reused in a single [`SpaceTime`].
    /// Calling a method with an invalidated snapshot id will cause a panic.
    pub fn drop_snapshot(&mut self, snapshot_id: SnapshotId) -> Result<(), InvalidSnapshotIdError> {
        self.snapshots.drop_snapshot(snapshot_id, &mut self.storage)
    }

    /// Returns an iterator over all available snapshots in no particular order.
    pub fn snapshots(&self) -> impl Iterator<Item = SnapshotId> + '_ {
        self.snapshots.snapshots()
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
        self.snapshots.contains(snapshot_id)
    }

    /// Returns the id of the checked out snapshot, or `None` if the HEAD is dirty.
    ///
    /// The HEAD is considered dirty if changes were written since the last [`Self::checkout`] or
    /// [`Self::make_snapshot`].
    pub fn head(&self) -> Option<SnapshotId> {
        self.snapshots.head_id()
    }

    /// Resets the state of HEAD to a snapshot.
    ///
    /// For now we assume all snapshots are stored forever if not explicitly dropped.
    pub fn checkout(&mut self, snapshot_id: SnapshotId) -> Result<(), InvalidSnapshotIdError> {
        self.snapshots.checkout(snapshot_id, &mut self.storage)
    }
}

impl Allocator for SpaceTime {
    type Id<T> = SpaceTimeId<T, false>;
    type ArrayId<T> = SpaceTimeId<T, true>;

    fn insert<T: Clone + 'static>(&mut self, object: T) -> Self::Id<T> {
        let (type_id, table) = self.storage.tables.get_or_default_mut();
        let table_ptr = table.add_item(object);

        let index = self
            .snapshots
            .get_head_mut(&mut self.storage)
            .add_table_ptr(table_ptr, type_id);
        SpaceTimeId::new(index)
    }

    fn insert_array<T: Copy + 'static>(&mut self, object: T, n: usize) -> Self::ArrayId<T> {
        let (type_id, table) = self.storage.array_storage.get_or_default_mut();
        let instance = table.new_instance(object, n as u64);

        let index = self
            .snapshots
            .get_head_mut(&mut self.storage)
            .add_instance(instance, type_id);
        SpaceTimeId::new(index)
    }

    /// See [`Allocator::remove`].
    ///
    /// If the object is only referenced by HEAD and not by any snapshot, it will be dropped.
    fn remove<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<(), InvalidIdError> {
        let table_ptr = self
            .snapshots
            .get_head_mut(&mut self.storage)
            .remove_table_ptr(id.index)
            .ok_or(InvalidIdError)?
            .table_ptr;

        let table = self
            .storage
            .tables
            .get_mut::<T>()
            .expect("T should be in the maps if there is a ptr in HEAD");

        table.drop_table_ptr(table_ptr);

        Ok(())
    }

    fn remove_array<T: Copy + 'static>(
        &mut self,
        id: Self::ArrayId<T>,
    ) -> Result<(), InvalidIdError> {
        let instance = self
            .snapshots
            .get_head_mut(&mut self.storage)
            .remove_instance(id.index)
            .ok_or(InvalidIdError)?
            .instance;

        let array_storage = self
            .storage
            .array_storage
            .get_mut::<T>()
            .expect("T should be in the storage_array if there is a ptr in HEAD");

        array_storage.remove_instance(instance);
        Ok(())
    }

    fn pop<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<T, InvalidIdError> {
        let table_ptr = self
            .snapshots
            .get_head_mut(&mut self.storage)
            .remove_table_ptr(id.index)
            .ok_or(InvalidIdError)?
            .table_ptr;

        let table = self
            .storage
            .tables
            .get_mut::<T>()
            .expect("T should be in the maps if there is a ptr in HEAD");

        Ok(table.pop_or_get_item(table_ptr))
    }

    fn get<T: Clone + 'static>(&self, id: Self::Id<T>) -> Result<&T, InvalidIdError> {
        let table_ptr = &self
            .snapshots
            .get_head()
            .get_table_ptr(id.index)
            .ok_or(InvalidIdError)?
            .table_ptr;

        let table = self
            .storage
            .tables
            .get::<T>()
            .expect("T should be in the maps if there is a ptr in HEAD");

        Ok(table.get_item(table_ptr))
    }

    fn get_array<T: Copy + 'static>(
        &self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessor<T>, InvalidIdError> {
        let instance = &self
            .snapshots
            .get_head()
            .get_instance(id.index)
            .ok_or(InvalidIdError)?
            .instance;

        let array_storage = self
            .storage
            .array_storage
            .get::<T>()
            .expect("T should be in the storage_array if there is a ptr in HEAD");

        Ok(STArrayAccessor {
            array_storage,
            instance,
        })
    }

    fn get_mut<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<&mut T, InvalidIdError> {
        let table_ptr = &mut self
            .snapshots
            .get_head_mut(&mut self.storage)
            .get_table_ptr_mut(id.index)
            .ok_or(InvalidIdError)?
            .table_ptr;

        let table = self
            .storage
            .tables
            .get_mut::<T>()
            .expect("T should be in the maps if there is a ptr in HEAD");

        if !table.is_unique_table_ptr(table_ptr) {
            // TablePtr clone is save as `clone_item` will drop the ptr given and we overwrite the
            // original here immediately.
            *table_ptr = table.clone_item(table_ptr.unsafe_clone(), T::clone);
        }

        // Unwrap safety: Either this already was a unique ptr, or we just cloned this page.
        Ok(table.get_item_mut(table_ptr).unwrap())
    }

    fn get_array_mut<T: Copy + 'static>(
        &mut self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessorMut<T>, InvalidIdError> {
        let instance = &mut self
            .snapshots
            .get_head_mut(&mut self.storage)
            .get_instance_mut(id.index)
            .ok_or(InvalidIdError)?
            .instance;

        let array_storage = self
            .storage
            .array_storage
            .get_mut::<T>()
            .expect("T should be in the storage_array if there is a ptr in HEAD");

        Ok(STArrayAccessorMut {
            array_storage,
            instance,
        })
    }
}

#[derive(Debug)]
struct STArrayAccessor<'a, T: Copy + 'static> {
    array_storage: &'a ArrayStorage<T>,
    instance: &'a Instance,
}

impl<T: Copy + 'static> ArrayAccessor<T> for STArrayAccessor<'_, T> {
    fn len(&self) -> usize {
        self.instance.len() as usize
    }

    fn get(&self, index: usize) -> Option<T> {
        self.instance.get(self.array_storage, index as u64).cloned()
    }

    fn get_ref(&self, index: usize) -> Option<&T> {
        self.instance.get(self.array_storage, index as u64)
    }

    fn read(&self, buf: &mut [T], index: usize) -> bool {
        self.instance.read(self.array_storage, buf, index as u64)
    }

    fn iter_range<R>(&self, index_range: R) -> Option<impl Iterator<Item = &T> + '_>
    where
        R: RangeBounds<usize>,
    {
        use std::ops::Bound;
        let start = match index_range.start_bound() {
            Bound::Unbounded => 0,
            Bound::Included(s) => *s as u64,
            Bound::Excluded(s) => (*s as u64).saturating_add(1),
        };
        let len = match index_range.end_bound() {
            Bound::Unbounded => self.instance.len().checked_sub(start)?,
            Bound::Included(e) => (*e as u64).checked_sub(start)? + 1,
            Bound::Excluded(e) => (*e as u64).checked_sub(start)?,
        };
        self.instance.iter_range(self.array_storage, start, len)
    }
}

#[derive(Debug)]
struct STArrayAccessorMut<'a, T: Copy + 'static> {
    array_storage: &'a mut ArrayStorage<T>,
    instance: &'a mut Instance,
}

impl<T: Copy + 'static> ArrayAccessor<T> for STArrayAccessorMut<'_, T> {
    fn len(&self) -> usize {
        self.instance.len() as usize
    }

    fn get(&self, index: usize) -> Option<T> {
        self.instance.get(self.array_storage, index as u64).cloned()
    }

    fn get_ref(&self, index: usize) -> Option<&T> {
        self.instance.get(self.array_storage, index as u64)
    }

    fn read(&self, buf: &mut [T], index: usize) -> bool {
        STArrayAccessor {
            array_storage: self.array_storage,
            instance: self.instance,
        }
        .read(buf, index)
    }

    fn iter_range<R>(&self, index_range: R) -> Option<impl Iterator<Item = &T> + '_>
    where
        R: RangeBounds<usize>,
    {
        use std::ops::Bound;
        let start = match index_range.start_bound() {
            Bound::Unbounded => 0,
            Bound::Included(s) => *s as u64,
            Bound::Excluded(s) => (*s as u64).saturating_add(1),
        };
        let end = match index_range.end_bound() {
            Bound::Unbounded => self.instance.len(),
            Bound::Included(e) => *e as u64,
            Bound::Excluded(e) => (*e as u64).saturating_sub(1),
        };
        if end >= self.instance.len() || start >= self.instance.len() {
            return None;
        }
        Some((start..end).map(|i| self.instance.get(self.array_storage, i).unwrap()))
    }
}

impl<T: Copy + 'static> ArrayAccessorMut<T> for STArrayAccessorMut<'_, T> {
    fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.instance.get_mut(self.array_storage, index as u64)
    }

    fn write(&mut self, index: usize, buf: &[T]) -> bool {
        self.instance.write(self.array_storage, index as u64, buf)
    }

    /// Files the whole array with the original value. In an optimized way
    fn reset(&mut self) {
        self.instance.reset(self.array_storage)
    }

    #[allow(unreachable_code)]
    fn iter_range_mut<R>(&mut self, _index_range: R) -> Option<impl Iterator<Item = &mut T> + '_>
    where
        R: RangeBounds<usize>,
    {
        todo!("Maybe someday :)") as Option<std::vec::IntoIter<&mut T>>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut sp = SpaceTime::new();

        let id1 = sp.insert(1u32);
        let id2 = sp.insert(2u32);

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));
        assert_eq!(sp.get(id1), Ok(&1u32));
    }

    #[test]
    fn insert_and_get_mixed_types() {
        let mut sp = SpaceTime::new();

        let id1 = sp.insert(1u32);
        let id2 = sp.insert(false);
        let id3 = sp.insert(2u32);
        let id4 = sp.insert(true);

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&false));
        assert_eq!(sp.get(id3), Ok(&2u32));
        assert_eq!(sp.get(id4), Ok(&true));
    }

    #[test]
    fn insert_and_remove() {
        let mut sp = SpaceTime::new();

        let id1 = sp.insert(1u32);
        let id2 = sp.insert(2u32);

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));

        sp.remove(id1).unwrap();
        sp.remove(id2).unwrap();

        assert_eq!(sp.get(id1), Err(InvalidIdError));
        assert_eq!(sp.get(id2), Err(InvalidIdError));

        let id3 = sp.insert(3u32);
        let id4 = sp.insert(4u32);

        assert_eq!(sp.get(id3), Ok(&3u32));
        assert_eq!(sp.get(id4), Ok(&4u32));

        assert_ne!(id1, id3);
        assert_ne!(id1, id4);
        assert_ne!(id2, id3);
        assert_ne!(id2, id4);
    }

    #[test]
    fn get_mut() {
        let mut sp = SpaceTime::new();

        let id1 = sp.insert(1u32);
        let id2 = sp.insert(2u32);

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));

        *sp.get_mut(id1).unwrap() = 3;
        *sp.get_mut(id2).unwrap() = 4;

        assert_eq!(sp.get(id1), Ok(&3u32));
        assert_eq!(sp.get(id2), Ok(&4u32));

        let id3 = sp.insert(3u32);
        let id4 = sp.insert(4u32);

        assert_eq!(sp.get(id3), Ok(&3u32));
        assert_eq!(sp.get(id4), Ok(&4u32));

        assert_ne!(id1, id3);
        assert_ne!(id1, id4);
        assert_ne!(id2, id3);
        assert_ne!(id2, id4);
    }

    #[test]
    fn simple_snapshot() {
        let mut sp = SpaceTime::new();

        let cp1 = sp.make_snapshot();

        let id1 = sp.insert(1u32);
        let id2 = sp.insert(2u32);

        let cp2 = sp.make_snapshot();

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));

        sp.checkout(cp1).unwrap();

        assert_eq!(sp.get(id1), Err(InvalidIdError));
        assert_eq!(sp.get(id2), Err(InvalidIdError));

        sp.checkout(cp2).unwrap();

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));
    }

    #[test]
    fn delete_snapshot() {
        let mut sp = SpaceTime::new();

        let cp1 = sp.make_snapshot();

        let id1 = sp.insert(1u32);
        let id2 = sp.insert(2u32);

        let cp2 = sp.make_snapshot();

        let id3 = sp.insert(3u32);
        let id4 = sp.insert(4u32);

        let cp3 = sp.make_snapshot();

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));
        assert_eq!(sp.get(id3), Ok(&3u32));
        assert_eq!(sp.get(id4), Ok(&4u32));

        sp.drop_snapshot(cp3).unwrap();

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));
        assert_eq!(sp.get(id3), Ok(&3u32));
        assert_eq!(sp.get(id4), Ok(&4u32));

        sp.checkout(cp1).unwrap();

        assert_eq!(sp.get(id1), Err(InvalidIdError));
        assert_eq!(sp.get(id2), Err(InvalidIdError));
        assert_eq!(sp.get(id3), Err(InvalidIdError));
        assert_eq!(sp.get(id4), Err(InvalidIdError));

        sp.checkout(cp2).unwrap();

        assert_eq!(sp.get(id1), Ok(&1u32));
        assert_eq!(sp.get(id2), Ok(&2u32));
        assert_eq!(sp.get(id3), Err(InvalidIdError));
        assert_eq!(sp.get(id4), Err(InvalidIdError));

        assert!(sp.checkout(cp3).is_err());
    }

    #[test]
    fn snapshot_id_ops() {
        let mut sp = SpaceTime::new();

        let cp1 = sp.make_snapshot();

        sp.insert(1u32);
        sp.insert(2u32);

        let cp2 = sp.make_snapshot();

        sp.insert(3u32);
        sp.insert(4u32);

        let cp3 = sp.make_snapshot();

        sp.drop_snapshot(cp2).unwrap();

        assert!(sp.has_snapshot(cp1));
        assert!(!sp.has_snapshot(cp2));
        assert!(sp.has_snapshot(cp3));

        let cps = sp.snapshots().collect::<Vec<_>>();
        assert!(cps.contains(&cp1));
        assert!(!cps.contains(&cp2));
        assert!(cps.contains(&cp3));
    }

    #[test]
    fn get_head() {
        let mut sp = SpaceTime::new();
        assert!(sp.head().is_none());

        let cp1 = sp.make_snapshot();

        assert_eq!(sp.head(), Some(cp1));

        sp.insert(1u32);

        assert!(sp.head().is_none());

        let cp2 = sp.make_snapshot();

        assert_eq!(sp.head(), Some(cp2));

        sp.insert(3u32);

        assert!(sp.head().is_none());

        let cp3 = sp.make_snapshot();

        assert_eq!(sp.head(), Some(cp3));

        sp.drop_snapshot(cp3).unwrap();

        assert!(sp.head().is_none());
    }

    #[test]
    fn get_mut_and_snapshots() {
        let mut sp = SpaceTime::new();

        let cp1 = sp.make_snapshot();

        let id = sp.insert(1u32);
        assert_eq!(sp.get(id), Ok(&1));

        let cp2 = sp.make_snapshot();

        *sp.get_mut(id).unwrap() = 2;

        let cp3 = sp.make_snapshot();

        *sp.get_mut(id).unwrap() = 3;

        sp.make_snapshot();

        assert_eq!(sp.get(id), Ok(&3));

        sp.checkout(cp1).unwrap();
        assert_eq!(sp.get(id), Err(InvalidIdError));

        sp.checkout(cp3).unwrap();
        assert_eq!(sp.get(id), Ok(&2));

        sp.checkout(cp2).unwrap();
        assert_eq!(sp.get(id), Ok(&1));
    }

    #[test]
    fn insert_array_and_get() {
        let mut sp = SpaceTime::new();

        let aid1 = sp.insert_array(0u8, 128);

        let ac1 = sp.get_array(aid1).unwrap();

        for i in 0..128 {
            assert_eq!(ac1.get(i), Some(0));
        }
        assert_eq!(ac1.get(128), None);
    }

    #[test]
    fn arrays_and_snapshots() {
        let mut sp = SpaceTime::new();

        let cp1 = sp.make_snapshot();

        let id1 = sp.insert_array(0u8, 64);

        let cp2 = sp.make_snapshot();

        let id2 = sp.insert_array(0u8, 64);

        let cp3 = sp.make_snapshot();

        assert!(sp.get_array(id1).is_ok());
        assert!(sp.get_array(id2).is_ok());

        sp.drop_snapshot(cp3).unwrap();

        assert!(sp.get_array(id1).is_ok());
        assert!(sp.get_array(id2).is_ok());

        sp.checkout(cp1).unwrap();

        assert!(sp.get_array(id1).is_err());
        assert!(sp.get_array(id2).is_err());

        sp.checkout(cp2).unwrap();

        assert!(sp.get_array(id1).is_ok());
        assert!(sp.get_array(id2).is_err());

        assert!(sp.checkout(cp3).is_err());
    }

    #[test]
    fn arrays_get_mut() {
        let mut sp = SpaceTime::new();

        let id1 = sp.insert_array(0u8, 128);
        let id2 = sp.insert_array(0u8, 128);

        let cp1 = sp.make_snapshot();

        let mut arr1 = sp.get_array_mut(id1).unwrap();
        *arr1.get_mut(1).unwrap() = 1;
        *arr1.get_mut(2).unwrap() = 2;
        drop(arr1);

        sp.make_snapshot();

        let mut arr2 = sp.get_array_mut(id2).unwrap();
        *arr2.get_mut(1).unwrap() = 1;
        *arr2.get_mut(2).unwrap() = 2;
        drop(arr2);

        let cp3 = sp.make_snapshot();

        let mut arr1 = sp.get_array_mut(id1).unwrap();
        *arr1.get_mut(71).unwrap() = 3;
        *arr1.get_mut(72).unwrap() = 4;
        drop(arr1);

        let cp4 = sp.make_snapshot();

        let mut arr1 = sp.get_array_mut(id1).unwrap();
        *arr1.get_mut(1).unwrap() = 3;
        *arr1.get_mut(2).unwrap() = 4;
        drop(arr1);

        sp.make_snapshot();

        let arr1 = sp.get_array(id1).unwrap();
        assert_eq!(arr1.get(1), Some(3));
        assert_eq!(arr1.get(2), Some(4));
        assert_eq!(arr1.get(71), Some(3));
        assert_eq!(arr1.get(72), Some(4));
        drop(arr1);
        let arr2 = sp.get_array(id2).unwrap();
        assert_eq!(arr2.get(1), Some(1));
        assert_eq!(arr2.get(2), Some(2));
        assert_eq!(arr2.get(72), Some(0));
        assert_eq!(arr2.get(72), Some(0));
        drop(arr2);

        sp.checkout(cp1).unwrap();
        let arr1 = sp.get_array(id1).unwrap();
        assert_eq!(arr1.get(1), Some(0));
        assert_eq!(arr1.get(2), Some(0));
        assert_eq!(arr1.get(71), Some(0));
        assert_eq!(arr1.get(72), Some(0));
        drop(arr1);
        let arr2 = sp.get_array(id2).unwrap();
        assert_eq!(arr2.get(1), Some(0));
        assert_eq!(arr2.get(2), Some(0));
        assert_eq!(arr2.get(72), Some(0));
        assert_eq!(arr2.get(72), Some(0));
        drop(arr2);

        sp.checkout(cp4).unwrap();
        let arr1 = sp.get_array(id1).unwrap();
        assert_eq!(arr1.get(1), Some(1));
        assert_eq!(arr1.get(2), Some(2));
        assert_eq!(arr1.get(71), Some(3));
        assert_eq!(arr1.get(72), Some(4));
        drop(arr1);
        let arr2 = sp.get_array(id2).unwrap();
        assert_eq!(arr2.get(1), Some(1));
        assert_eq!(arr2.get(2), Some(2));
        assert_eq!(arr2.get(72), Some(0));
        assert_eq!(arr2.get(72), Some(0));
        drop(arr2);

        sp.checkout(cp3).unwrap();
        let arr1 = sp.get_array(id1).unwrap();
        assert_eq!(arr1.get(1), Some(1));
        assert_eq!(arr1.get(2), Some(2));
        assert_eq!(arr1.get(71), Some(0));
        assert_eq!(arr1.get(72), Some(0));
        drop(arr1);
        let arr2 = sp.get_array(id2).unwrap();
        assert_eq!(arr2.get(1), Some(1));
        assert_eq!(arr2.get(2), Some(2));
        assert_eq!(arr2.get(72), Some(0));
        assert_eq!(arr2.get(72), Some(0));
        drop(arr2);
    }

    #[test]
    fn array_buf() {
        let mut sp = SpaceTime::new();

        let id = sp.insert_array(0u8, 128);

        let mut arr = sp.get_array_mut(id).unwrap();
        assert!(arr.write(1, &[1, 2, 3, 4, 5, 6, 7, 8, 9]));
        assert!(arr.write(60, &[1, 2, 3, 4, 5, 6, 7, 8, 9]));
        drop(arr);

        let arr = sp.get_array(id).unwrap();

        let mut buf = [0; 128];
        assert!(arr.read(&mut buf, 0));
        assert!(!arr.read(&mut buf, 1));

        let mut buf = [0; 10];
        assert!(arr.read(&mut buf, 1));
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9, 0]);
        assert!(arr.read(&mut buf, 60));
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9, 0]);
    }

    #[test]
    fn array_buf_with_snapshots() {
        let mut sp = SpaceTime::new();

        let id = sp.insert_array(0u8, 128);

        let cp1 = sp.make_snapshot();

        let mut arr = sp.get_array_mut(id).unwrap();
        assert!(arr.write(1, &[1, 2, 3, 4, 5, 6, 7, 8, 9]));
        assert!(arr.write(60, &[1, 2, 3, 4, 5, 6, 7, 8, 9]));
        drop(arr);

        let cp2 = sp.make_snapshot();

        sp.checkout(cp1).unwrap();

        let arr = sp.get_array(id).unwrap();
        let mut buf = [0; 128];
        assert!(arr.read(&mut buf, 0));
        assert!(buf.iter().all(|x| *x == 0));
        drop(arr);

        sp.checkout(cp2).unwrap();

        let arr = sp.get_array(id).unwrap();

        let mut buf = [0; 128];
        assert!(arr.read(&mut buf, 0));
        assert!(!arr.read(&mut buf, 1));

        let mut buf = [0; 10];
        assert!(arr.read(&mut buf, 1));
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9, 0]);
        assert!(arr.read(&mut buf, 60));
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9, 0]);
    }

    #[test]
    fn array_iter() {
        let mut sp = SpaceTime::new();

        let id = sp.insert_array(0u8, 128);

        let mut arr = sp.get_array_mut(id).unwrap();
        let _ = arr.write(1, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        drop(arr);

        let arr = sp.get_array(id).unwrap();

        assert!(arr.iter_range(0..=128).is_none());
        assert!(arr.iter_range(0..129).is_none());

        for i in arr.iter_range(1..10).unwrap() {
            dbg!(i);
        }

        assert!(arr.iter_range(1..10).unwrap().cloned().eq(1..10));
    }
}
