use std::mem::MaybeUninit;

use downcast_rs::{impl_downcast, Downcast};

/// Ptr into the next table.
#[derive(Debug, PartialEq, Eq)]
#[repr(transparent)]
#[must_use = "When droping a page ptr, make sure you notified the Table it belongs to."]
pub(crate) struct TablePtr(u32);

impl TablePtr {
    fn as_usize(&self) -> usize {
        self.0 as usize
    }

    /// The caller should make sure only one of the two TablePtr's lives on, and *not* using the
    /// drop methods on [`Table`] for the other one.
    pub(crate) fn unsafe_clone(&self) -> TablePtr {
        TablePtr(self.0)
    }
}

/// Either stores a pointer to the next empty page, or counts the amount of references to this block
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ItemMetaData(u32);

impl std::fmt::Debug for ItemMetaData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_filled() {
            f.debug_tuple("Used").field(&self.ref_count()).finish()
        } else {
            f.debug_tuple("Empty").field(&self.next_empty()).finish()
        }
    }
}

const EMPTY_MASK: u32 = 0x80000000;

impl ItemMetaData {
    /// Creates a new [`TablePageMetaData`] with indication a single reference.
    fn filled() -> ItemMetaData {
        ItemMetaData(1)
    }

    fn empty(empty: u32) -> ItemMetaData {
        let i = ItemMetaData(empty | EMPTY_MASK);
        debug_assert_eq!(i.next_empty(), empty);
        i
    }

    fn is_filled(&self) -> bool {
        (self.0 & EMPTY_MASK) == 0
    }

    /// Adds one extra ref. Returning the refs left.
    fn add_ref(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }

    /// Removes a ref. Returning the refs left.
    fn remove_ref(&mut self) -> u32 {
        self.0 -= 1;
        self.0
    }

    /// Gets the ref count.
    fn ref_count(&self) -> u32 {
        self.0
    }

    /// Gets the ptr to the next empty.
    fn next_empty(&self) -> u32 {
        self.0 & !EMPTY_MASK
    }
}

/// Growable refcounted slab of items.
#[derive(Debug)]
pub(crate) struct Table<T: 'static> {
    metadata: Vec<ItemMetaData>,
    table: Vec<MaybeUninit<T>>,
    next_empty: u32,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> Table<T> {
    pub(crate) fn new() -> Self {
        Self {
            metadata: Vec::new(),
            table: Vec::new(),
            next_empty: 0,
        }
    }

    /// Adds a new page into the table, with a single reference counted.
    pub(crate) fn add_item(&mut self, item: T) -> TablePtr {
        // TODO: Find earlier empty page
        let index: u32 = self.next_empty;
        if index as usize == self.table.len() {
            self.table.push(MaybeUninit::new(item));
            self.metadata.push(ItemMetaData::filled());
            self.next_empty = self.next_empty.checked_add(1).expect("Table full");
        } else {
            let index = index as usize;
            self.next_empty = self.metadata[index].next_empty();
            self.metadata[index] = ItemMetaData::filled();

            self.table[index] = MaybeUninit::new(item);
        }
        TablePtr(index)
    }

    pub(crate) fn get_item(&self, table_ptr: &TablePtr) -> &T {
        if !self.metadata[table_ptr.as_usize()].is_filled() {
            panic!("Invalid table_ptr");
        }
        unsafe { self.table[table_ptr.as_usize()].assume_init_ref() }
    }

    /// Clones the page, giving a ptr to the new page. This will be a page
    /// with a single reference counted. The old page will also have its
    /// ref count decreased by one.
    pub(crate) fn clone_item<F>(&mut self, table_ptr: TablePtr, mut clone_item: F) -> TablePtr
    where
        F: FnMut(&T) -> T,
    {
        let item = self.get_item(&table_ptr);
        let ptr = self.add_item(clone_item(item));

        self.drop_table_ptr(table_ptr);

        ptr
    }

    /// Deletes item from this table. If it was the last ptr to reference this item, it gets
    /// returned.
    pub(crate) fn pop_item(&mut self, table_ptr: TablePtr) -> Option<T> {
        if !self.metadata[table_ptr.as_usize()].is_filled() {
            panic!("Invalid table_ptr");
        }

        let index = table_ptr.as_usize();
        let refs_left = self.metadata[index].remove_ref();
        if refs_left == 0 {
            self.metadata[index] = ItemMetaData::empty(self.next_empty);
            self.next_empty = table_ptr.0;

            // Safety: We checked that the metadata said this cell is used before so it is
            // guaranteed to be initialized. It is also safe to make a bitwise copy as we just set
            // the metadata to empty, disallowing any further reads.
            return Some(unsafe { self.table[index].assume_init_read() });
        }
        None
    }

    /// Removes ref from this table. If it was the last ptr to reference this item, it gets
    /// returned, otherwise the current value gets cloned.
    pub(crate) fn pop_or_get_item(&mut self, table_ptr: TablePtr) -> T
    where
        T: Clone,
    {
        let index = table_ptr.as_usize();
        match self.pop_item(table_ptr) {
            Some(t) => t,
            None => {
                // Safety: pop_item will have checked if the table_ptr is valid, making it save
                // to call assume_init_ref.
                unsafe { self.table[index].assume_init_ref() }.clone()
            }
        }
    }

    pub(crate) fn clone_table_ptr_array<const N: usize>(
        &mut self,
        table_ptr: TablePtr,
    ) -> [TablePtr; N] {
        // IDEA: This can be a specialized method not using `clone_page_ptr` for better performance.
        let out = [&table_ptr; N].map(|p| self.clone_table_ptr(p));
        self.drop_table_ptr(table_ptr);
        out
    }

    /// Returns the item if the ref count is 1, Returns [`None`] other wise.
    pub(crate) fn get_item_mut(&mut self, table_ptr: &TablePtr) -> Option<&mut T> {
        if self.is_unique_table_ptr(table_ptr) {
            // Safety: is_unique_table_ptr is only true if this cell is not empty, making it safe
            // to assume it is initialized.
            return Some(unsafe { self.table[table_ptr.as_usize()].assume_init_mut() });
        }
        None
    }
}

impl<T: 'static, const N: usize> Table<[T; N]> {
    /// Clones the page, giving a ptr to the new page. This will be a page
    /// with a single reference counted. The old page will also have its
    /// ref count decreased by one.
    pub(crate) fn clone_array_item<F>(&mut self, table_ptr: TablePtr, clone_item: F) -> TablePtr
    where
        F: FnMut(&T) -> T,
    {
        let item = self.get_item(&table_ptr);
        let item = item.each_ref().map(clone_item);
        let ptr = self.add_item(item);

        self.drop_table_ptr(table_ptr);

        ptr
    }
}

pub(crate) trait TableTrait: Downcast {
    /// Clone a TablePtr adding an extra reference to the item.
    fn clone_table_ptr(&mut self, table_ptr: &TablePtr) -> TablePtr;

    fn is_unique_table_ptr(&mut self, table_ptr: &TablePtr) -> bool;

    fn drop_table_ptr(&mut self, table_ptr: TablePtr);
}
impl_downcast!(TableTrait);

impl<T: 'static> TableTrait for Table<T> {
    fn clone_table_ptr(&mut self, table_ptr: &TablePtr) -> TablePtr {
        self.metadata[table_ptr.as_usize()].add_ref();
        TablePtr(table_ptr.0)
    }

    fn is_unique_table_ptr(&mut self, table_ptr: &TablePtr) -> bool {
        let metadata = &self.metadata[table_ptr.as_usize()];
        if !metadata.is_filled() {
            return false;
        }
        let ref_count = metadata.ref_count();
        ref_count == 1
    }

    fn drop_table_ptr(&mut self, table_ptr: TablePtr) {
        self.pop_item(table_ptr);
    }
}

impl std::fmt::Debug for (dyn TableTrait + 'static) {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("dyn TableTrait").finish()
    }
}
