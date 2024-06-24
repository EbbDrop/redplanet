use nohash::IntMap;

use crate::{
    array_storage::{ArrayStorage, ArrayStorageTrait},
    table::{Table, TableTrait},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TypeId(std::any::TypeId);

impl TypeId {
    pub fn of<T: ?Sized + 'static>() -> Self {
        Self(std::any::TypeId::of::<T>())
    }
}

impl nohash::IsEnabled for TypeId {}

#[derive(Debug, Default)]
pub(crate) struct TableTypeMap(IntMap<TypeId, Box<dyn TableTrait>>);

impl TableTypeMap {
    pub(crate) fn get_with_id(&self, type_id: TypeId) -> Option<&dyn TableTrait> {
        self.0.get(&type_id).map(|a| &**a)
    }

    pub(crate) fn get_with_id_mut(&mut self, type_id: TypeId) -> Option<&mut dyn TableTrait> {
        self.0.get_mut(&type_id).map(|a| &mut **a)
    }

    pub(crate) fn get<T: 'static>(&self) -> Option<&Table<T>> {
        let type_id = TypeId::of::<T>();
        let b = self.get_with_id(type_id)?;

        Some(
            b.downcast_ref()
                .expect("HashMap should never contain type not coresponding to its key"),
        )
    }

    pub(crate) fn get_mut<T: 'static>(&mut self) -> Option<&mut Table<T>> {
        let type_id = TypeId::of::<T>();
        let b = self.get_with_id_mut(type_id)?;

        Some(
            b.downcast_mut()
                .expect("HashMap should never contain type not coresponding to its key"),
        )
    }

    /// Get a certain types table from the map. If this type is not available in the map, the
    /// default table gets inserted and that is returned.
    pub(crate) fn get_or_default_mut<T: 'static>(&mut self) -> (TypeId, &mut Table<T>) {
        let type_id = TypeId::of::<T>();

        let b = self
            .0
            .entry(type_id)
            .or_insert_with(|| Box::<Table<T>>::default());

        let table = b
            .downcast_mut()
            .expect("HashMap should never contain type not coresponding to its key");
        (type_id, table)
    }
}

#[derive(Debug, Default)]
pub(crate) struct ArrayStorageTypeMap(IntMap<TypeId, Box<dyn ArrayStorageTrait>>);

impl ArrayStorageTypeMap {
    pub(crate) fn get_with_id(&self, type_id: TypeId) -> Option<&dyn ArrayStorageTrait> {
        self.0.get(&type_id).map(|a| &**a)
    }

    pub(crate) fn get_with_id_mut(
        &mut self,
        type_id: TypeId,
    ) -> Option<&mut dyn ArrayStorageTrait> {
        self.0.get_mut(&type_id).map(|a| &mut **a)
    }

    pub(crate) fn get<T: Copy + 'static>(&self) -> Option<&ArrayStorage<T>> {
        let type_id = TypeId::of::<T>();
        let b = self.get_with_id(type_id)?;

        Some(
            b.downcast_ref()
                .expect("HashMap should never contain type not coresponding to its key"),
        )
    }

    pub(crate) fn get_mut<T: Copy + 'static>(&mut self) -> Option<&mut ArrayStorage<T>> {
        let type_id = TypeId::of::<T>();
        let b = self.get_with_id_mut(type_id)?;

        Some(
            b.downcast_mut()
                .expect("HashMap should never contain type not coresponding to its key"),
        )
    }

    pub(crate) fn get_or_default_mut<T: Copy + 'static>(
        &mut self,
    ) -> (TypeId, &mut ArrayStorage<T>) {
        let type_id = TypeId::of::<T>();

        let b = self
            .0
            .entry(type_id)
            .or_insert_with(|| Box::<ArrayStorage<T>>::default());

        let table = b
            .downcast_mut()
            .expect("HashMap should never contain type not coresponding to its key");
        (type_id, table)
    }
}
