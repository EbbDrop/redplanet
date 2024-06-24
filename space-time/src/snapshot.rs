use crate::typemap::TypeId;

use generational_arena::Index;

use crate::{array_storage::Instance, table::TablePtr};

/// We have to store the TypeId, for the TablePtr's to know what table in the TypeMap the ref
/// count needs to be increased/decreased in when cloning/removing snapshot.
#[derive(Debug)]
pub(crate) struct TypedTablePtr {
    pub(crate) table_ptr: TablePtr,
    pub(crate) type_id: TypeId,
}

#[derive(Debug)]
pub(crate) struct TypedInstance {
    pub(crate) instance: Instance,
    pub(crate) type_id: TypeId,
}

// IDEA: A possible optimization would be to use our own generational arena here. Every entry in the
// libraries uses 16 bytes extra because of padding. When making our own the generation and the tag
// for a free or occupied entry could be placed in the padding of TypedTablePtr by Rust.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    table_ptrs: generational_arena::Arena<TypedTablePtr>,
    instances: generational_arena::Arena<TypedInstance>,
}

impl Snapshot {
    pub(crate) fn from_iterators<IT, II>(table_ptrs: IT, instances: II) -> Snapshot
    where
        IT: Iterator<Item = TypedTablePtr>,
        II: Iterator<Item = TypedInstance>,
    {
        Self {
            table_ptrs: table_ptrs.collect(),
            instances: instances.collect(),
        }
    }

    pub(crate) fn add_table_ptr(&mut self, table_ptr: TablePtr, type_id: TypeId) -> Index {
        self.table_ptrs.insert(TypedTablePtr { table_ptr, type_id })
    }

    pub(crate) fn get_table_ptr(&self, index: Index) -> Option<&TypedTablePtr> {
        self.table_ptrs.get(index)
    }

    pub(crate) fn get_table_ptr_mut(&mut self, index: Index) -> Option<&mut TypedTablePtr> {
        self.table_ptrs.get_mut(index)
    }

    pub(crate) fn remove_table_ptr(&mut self, index: Index) -> Option<TypedTablePtr> {
        self.table_ptrs.remove(index)
    }

    pub(crate) fn iter_table_ptrs(&self) -> impl Iterator<Item = &TypedTablePtr> {
        self.table_ptrs.iter().map(|(_index, e)| e)
    }

    pub(crate) fn add_instance(&mut self, instance: Instance, type_id: TypeId) -> Index {
        self.instances.insert(TypedInstance { instance, type_id })
    }

    pub(crate) fn get_instance(&self, index: Index) -> Option<&TypedInstance> {
        self.instances.get(index)
    }

    pub(crate) fn get_instance_mut(&mut self, index: Index) -> Option<&mut TypedInstance> {
        self.instances.get_mut(index)
    }

    pub(crate) fn remove_instance(&mut self, index: Index) -> Option<TypedInstance> {
        self.instances.remove(index)
    }

    pub(crate) fn iter_instances(&self) -> impl Iterator<Item = &TypedInstance> {
        self.instances.iter().map(|(_index, i)| i)
    }

    pub(crate) fn into_iterators(
        self,
    ) -> (
        impl Iterator<Item = TypedTablePtr>,
        impl Iterator<Item = TypedInstance>,
    ) {
        let Self {
            table_ptrs,
            instances,
        } = self;
        (table_ptrs.into_iter(), instances.into_iter())
    }
}
