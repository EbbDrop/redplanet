use std::{any::type_name, fmt::Debug, hash::Hash, marker::PhantomData};

use generational_arena::Index;

pub struct SpaceTimeId<T, const ARRAY: bool> {
    pub(crate) index: Index,
    _phan: PhantomData<T>,
}

impl<T, const ARRAY: bool> SpaceTimeId<T, ARRAY> {
    pub(crate) fn new(index: Index) -> Self {
        Self {
            index,
            _phan: PhantomData,
        }
    }
}

impl<T, const ARRAY: bool> Debug for SpaceTimeId<T, ARRAY> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpaceTimeId")
            .field("index", &self.index)
            .field("type", &type_name::<T>())
            .field("array", &if ARRAY { "yes" } else { "no" })
            .finish()
    }
}

impl<T, const ARRAY: bool> Clone for SpaceTimeId<T, ARRAY> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, const ARRAY: bool> Copy for SpaceTimeId<T, ARRAY> {}

impl<T, const ARRAY: bool> PartialEq for SpaceTimeId<T, ARRAY> {
    fn eq(&self, other: &Self) -> bool {
        self.index.eq(&other.index)
    }
}

impl<T, const ARRAY: bool> Eq for SpaceTimeId<T, ARRAY> {}

impl<T, const ARRAY: bool> PartialOrd for SpaceTimeId<T, ARRAY> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T, const ARRAY: bool> Ord for SpaceTimeId<T, ARRAY> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

impl<T, const ARRAY: bool> Hash for SpaceTimeId<T, ARRAY> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}
