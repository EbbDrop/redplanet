use std::{fmt::Debug, ops::RangeBounds};

use crate::errors::InvalidIdError;

/// Trait for types that provide both access and insertion/removal capabilities.
pub trait Allocator: Debug {
    /// Type used to identify objects of type `T`.
    ///
    /// For safety, once an object with an id is removed, that id should never be used again by the
    /// same [`Allocator`].
    type Id<T>: Copy + Eq + Debug;

    /// Type used to identify arrays of objects of type `T`.
    ///
    /// For safety, once an array with an id is removed, that id should never be used again by the
    /// same [`Allocator`].
    type ArrayId<T>: Copy + Eq + Debug;

    /// Inserts an object of type `T`.
    ///
    /// Note that the size of `T` should be kept small, because the granularity of deduplication
    /// between snapshots is the size of `T`. In particular, when requiring allocation of an array
    /// or tuple, prefer allocating each element individually rather than the array or tuple as a
    /// whole. This allows snapshots to share the value of other elements if only one element
    /// changed between snapshots. On the other hand, if most elements of the array or tuple are
    /// mostly changed at once, it could be better to allocate the array or tuple as a whole, as
    /// this improves locality, and reduces the indirection overhead in the snapshots internally.
    fn insert<T: Clone + 'static>(&mut self, object: T) -> Self::Id<T>;

    /// Inserts an array of `n` of objects of type `T`, initialized with copies of `object`.
    ///
    /// The copies will be addressable using indices in the range `0..n`.
    ///
    /// See also [`ArrayAccessor`] and [`ArrayAccessorMut`].
    fn insert_array<T: Copy + 'static>(&mut self, object: T, n: usize) -> Self::ArrayId<T>;

    /// Removes an object of type `T`.
    ///
    /// If you need an owned version of the removed object, use [`Self::pop`].
    ///
    /// This is not guaranteed to trigger a [`Drop::drop`] of the `T` object.
    fn remove<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<(), InvalidIdError>;

    /// Removes an array of objects of type `T`.
    ///
    /// There is no equivalent of [`Self::pop`] for arrays, since `T` must be `Copy`.
    ///
    /// If you need the values, consider retrieving them first using the available accessors.
    fn remove_array<T: Copy + 'static>(
        &mut self,
        id: Self::ArrayId<T>,
    ) -> Result<(), InvalidIdError>;

    /// Removes an object of type `T` and returns on owned version.
    ///
    /// Note that this might return a clone of the object originally passed to [`Self::insert`].
    ///
    /// This will never trigger a [`Drop::drop`] of the `T` object.
    fn pop<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<T, InvalidIdError>;

    /// Acquire a reference to an object of type `T` by id.
    fn get<T: Clone + 'static>(&self, id: Self::Id<T>) -> Result<&T, InvalidIdError>;

    /// Get an accessor object to be able to index immutably inside an array.
    fn get_array<T: Copy + 'static>(
        &self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessor<T>, InvalidIdError>;

    /// Acquire a mutable reference to an object of type `T` by id.
    ///
    /// This operation may be very expensive. Only use this if you are absolutely sure you will
    /// certainly need a *mutable* reference. Otherwise consider using [`Self::get`] which never has
    /// this increased cost.
    fn get_mut<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<&mut T, InvalidIdError>;

    /// Get an accessor object to be able to index mutably inside an array.
    ///
    /// Note that the operations available on the [`ArrayAccessorMut`] are often very expensive.
    /// Use [`Self::get_array`] when you can, but it is ok to use this method even if you are not
    /// yet sure you will need to invoke a mutable method on [`ArrayAccessorMut`], since this method
    /// itself is not costlier than [`Self::get_array`].
    fn get_array_mut<T: Copy + 'static>(
        &mut self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessorMut<T>, InvalidIdError>;
}

pub trait ArrayAccessor<T: Copy + 'static> {
    /// Returns the number of objects in the array this [`ArrayAccessor`] provides access to.
    fn len(&self) -> usize;

    // This method is mostly here to satisfy clippy
    /// Returns `true` if `self.len() == 0`.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a copy of the object at `index`.
    /// Returns `None` if `index` is out of bounds.
    fn get(&self, index: usize) -> Option<T>;

    /// Returns a reference to the object at `index`.
    /// Returns `None` if `index` is out of bounds.
    fn get_ref(&self, index: usize) -> Option<&T>;

    /// Reads the objects in the array starting at `index` into `buf`.
    ///
    /// Returns `False` if `index` is out of bounds or if there are not `buf.len()` objects available
    /// to read (starting at `index`).
    #[must_use = "A return value of false is probably a bad sign"]
    fn read(&self, buf: &mut [T], index: usize) -> bool;

    /// Returns an iterator over references to the objects in a range of indices.
    /// Returns `None` if the range is not entirely within bounds.
    ///
    /// The allowed ranges follow the std rules for indexing a slice using a range.
    fn iter_range<R>(&self, index_range: R) -> Option<impl Iterator<Item = &T> + '_>
    where
        R: RangeBounds<usize>;
}

pub trait ArrayAccessorMut<T: Copy + 'static>: ArrayAccessor<T> {
    /// Returns a mutable reference to the object at `index`.
    /// Returns `None` if `index` is out of bounds.
    ///
    /// Only use this if you are absolutely sure you will certainly need *mutable* access.
    /// Otherwise consider using [`ArrayAccessor::get`] or [`ArrayAccessor::get_ref`] which never
    /// have this increased cost.
    fn get_mut(&mut self, index: usize) -> Option<&mut T>;

    /// Sets the object at `index` to `value`.
    /// Returns `false` if `index` is out of bounds.
    #[must_use = "A return value of false is probably a bad sign"]
    fn set(&mut self, index: usize, value: T) -> bool {
        let Some(v) = self.get_mut(index) else {
            return false;
        };

        *v = value;

        true
    }

    /// Writes the objects from `buf` to the array starting at `index`.
    ///
    /// Returns `false` if `index` is out of bounds or if there are not `buf.len()` object spaces
    /// remaining (starting at `index`).
    #[must_use = "A return value of false is probably a bad sign"]
    fn write(&mut self, index: usize, buf: &[T]) -> bool;

    /// Files the whole array with the original value. In an optimized way.
    ///
    /// This function always succeeds.
    fn reset(&mut self);

    /// Returns an iterator over mutable references to the objects in a range of indices.
    /// Returns `None` if the range is not entirely within bounds.
    ///
    /// The allowed ranges follow the std rules for indexing a slice using a range.
    ///
    /// This operation may be extremely expensive. Only use it if you certainly need a *mutable*
    /// reference to *all* or most of the items that are iterated over. The wider the `index_range`,
    /// the higher the cost of this operation can be.
    fn iter_range_mut<R>(&mut self, index_range: R) -> Option<impl Iterator<Item = &mut T> + '_>
    where
        R: RangeBounds<usize>;
}
