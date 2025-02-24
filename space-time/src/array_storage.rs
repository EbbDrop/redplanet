use downcast_rs::{impl_downcast, Downcast};

use crate::table::{Table, TablePtr, TableTrait};

const PAGE_SIZE: usize = 64;

pub(crate) trait ArrayStorageTrait: Downcast {
    fn clone_instance(&mut self, instance: &Instance) -> Instance;

    fn drop_instance(&mut self, instance: Instance);
}
impl_downcast!(ArrayStorageTrait);

#[derive(Debug)]
pub(crate) struct ArrayStorage<T: Copy + 'static> {
    data_table: Table<[T; PAGE_SIZE]>,
    /// First table is at the bottom, last table is the one with pointers to
    /// the `data_table`.
    page_tables: Box<[Table<[TablePtr; PAGE_SIZE]>]>,
}

impl<T: Copy> Default for ArrayStorage<T> {
    fn default() -> Self {
        // 3 layers chosen so only 256 PagePtr would be needed to store 4GB of u32's.
        Self::new(3)
    }
}

impl<T: Copy> ArrayStorage<T> {
    /// Creates a new [`DataStorage`] having `layers` amount of redirection
    /// to the last layer. This ArrayStorage will be completely empty.
    fn new(layers: usize) -> Self {
        let mut page_tables = Vec::with_capacity(layers);
        for _ in 0..layers {
            page_tables.push(Table::new());
        }

        Self {
            data_table: Table::default(),
            page_tables: page_tables.into_boxed_slice(),
        }
    }

    fn add_top_level_page(&mut self, value: T) -> TablePtr {
        let mut prev_table_ptr = self.data_table.add_item([value; PAGE_SIZE]);
        // None indicates the data_table
        let mut prev_table: Option<&mut Table<[TablePtr; PAGE_SIZE]>> = None;

        for table in self.page_tables.iter_mut().rev() {
            let page = match prev_table {
                Some(prev_table) => prev_table.clone_table_ptr_array(prev_table_ptr),
                None => self.data_table.clone_table_ptr_array(prev_table_ptr),
            };

            prev_table_ptr = table.add_item(page);
            prev_table = Some(table);
        }

        prev_table_ptr
    }

    /// Creates a new [`Instance`] for this [`DataStorage`] holding `size` amount of `T`'s
    /// Initiated with `T::default()`.
    pub(crate) fn new_instance(&mut self, value: T, size: u64) -> Instance {
        let table_ptr = self.add_top_level_page(value);

        let pages_needed = size.div_ceil(self.items_per_top_level_page());

        let pages: Vec<_> = (0..pages_needed)
            .map(|_| self.clone_table_ptr(&table_ptr))
            .collect();
        let pages = pages.into_boxed_slice();

        Instance {
            pages,
            size,
            reset_page: table_ptr,
        }
    }

    pub(crate) fn remove_instance(&mut self, instance: Instance) {
        for table_ptr in instance.pages.into_vec().into_iter() {
            self.drop_table_ptr(table_ptr);
        }
    }

    fn items_per_top_level_page(&self) -> u64 {
        let total_layers = self.page_tables.len() + 1;
        (PAGE_SIZE as u64).pow(total_layers as u32)
    }

    fn get(&self, table_ptr: &TablePtr, index: u64) -> &T {
        let mut table_ptr = table_ptr;
        let mut index = index;

        let mut divisor = self.items_per_top_level_page() / PAGE_SIZE as u64;

        for table in self.page_tables.iter() {
            let page = table.get_item(table_ptr);
            let i = index / divisor;

            table_ptr = &page[i as usize];
            index -= i * divisor;
            divisor /= PAGE_SIZE as u64;
        }
        debug_assert_eq!(divisor, 1);

        let data_page = self.data_table.get_item(table_ptr);
        &data_page[index as usize]
    }

    fn read_impl(
        &self,
        buf: &mut [T],
        table_ptr: &TablePtr,
        index: u64,
        table_depth: usize,
        divisor: u64,
    ) {
        let first_page = index / divisor;
        let last_page = (index + buf.len() as u64).div_ceil(divisor);

        let page_index = index - (first_page * divisor);

        let amount_of_pages = last_page - first_page;

        if let Some(table) = self.page_tables.get(table_depth) {
            let tables = table.get_item(table_ptr);

            for i in 0..amount_of_pages {
                let start = (i * divisor).saturating_sub(page_index);

                let end = (i + 1) * divisor - page_index;
                let end = end.min(buf.len() as u64);

                let buf = &mut buf[(start as usize)..(end as usize)];

                let table_ptr = &tables[(first_page + i) as usize];

                let index = if i == 0 { page_index } else { 0 };

                self.read_impl(
                    buf,
                    table_ptr,
                    index,
                    table_depth + 1,
                    divisor / PAGE_SIZE as u64,
                );
            }
        } else {
            debug_assert!(buf.len() <= PAGE_SIZE - index as usize);
            debug_assert!(divisor == 1);
            let index = index as usize;

            let page = self.data_table.get_item(table_ptr);

            buf.clone_from_slice(&page[(index)..(index + buf.len())]);
        }
    }

    fn read(&self, buf: &mut [T], table_ptr: &TablePtr, index: u64) {
        let divisor = self.items_per_top_level_page() / PAGE_SIZE as u64;

        self.read_impl(buf, table_ptr, index, 0, divisor);
    }

    fn iter_range_impl(
        &self,
        table_ptr: &TablePtr,
        index: u64,
        len: u64,
        table_depth: usize,
        divisor: u64,
    ) -> Box<dyn Iterator<Item = &'_ T> + '_> {
        let first_page = index / divisor;
        let last_page = (index + len).div_ceil(divisor);

        let page_index = index - (first_page * divisor);

        let amount_of_pages = last_page - first_page;

        if let Some(table) = self.page_tables.get(table_depth) {
            let tables = table.get_item(table_ptr);

            let iter = (0..amount_of_pages).flat_map(move |i| {
                let table_ptr = &tables[(first_page + i) as usize];

                let index = if i == 0 { page_index } else { 0 };

                let len_left = len - (i * divisor).saturating_sub(page_index);

                self.iter_range_impl(
                    table_ptr,
                    index,
                    (divisor - index).min(len_left),
                    table_depth + 1,
                    divisor / PAGE_SIZE as u64,
                )
            });
            Box::new(iter)
        } else {
            debug_assert!(len <= PAGE_SIZE as u64 - index);
            debug_assert!(divisor == 1);
            let index = index as usize;
            let end = index + len as usize;

            let page = self.data_table.get_item(table_ptr);

            Box::new(page[index..end].iter())
        }
    }

    fn iter_range(
        &self,
        table_ptr: &TablePtr,
        index: u64,
        len: u64,
    ) -> impl Iterator<Item = &'_ T> {
        let divisor = self.items_per_top_level_page() / PAGE_SIZE as u64;

        self.iter_range_impl(table_ptr, index, len, 0, divisor)
    }

    fn get_mut(&mut self, table_ptr: &mut TablePtr, index: u64) -> &mut T {
        let mut table_ptr = table_ptr;
        let mut index = index;

        let mut divisor = self.items_per_top_level_page() / PAGE_SIZE as u64;

        let mut iter = self.page_tables.iter_mut().peekable();
        while let Some(table) = iter.next() {
            if !table.is_unique_table_ptr(table_ptr) {
                // IDEA: Not using the box is possible by doing the clone inside the match, generating slightly better
                // optimized code. At the cost of readability.

                // Look for next item in the iterator to find the table to clone the ptr's in.
                // If this is the last table, we have to clone inside the data_table.
                let cloner: Box<dyn FnMut(&TablePtr) -> TablePtr> = match iter.peek_mut() {
                    Some(next_table) => Box::new(|ptr| next_table.clone_table_ptr(ptr)),
                    None => Box::new(|ptr| self.data_table.clone_table_ptr(ptr)),
                };
                // TablePtr clone is save as `clone_item` will drop the ptr given and we overwrite the
                // original here immediately.
                *table_ptr = table.clone_array_item(table_ptr.unsafe_clone(), cloner);
            }

            // Unwrap safety: Either this already was a unique ptr, or we just cloned this page.
            let page = table.get_item_mut(table_ptr).unwrap();
            let i = index / divisor;

            table_ptr = &mut page[i as usize];
            index -= i * divisor;
            divisor /= PAGE_SIZE as u64;
        }
        debug_assert_eq!(divisor, 1);

        if !self.data_table.is_unique_table_ptr(table_ptr) {
            *table_ptr = self
                .data_table
                .clone_array_item(table_ptr.unsafe_clone(), T::clone);
        }
        let data_page = self.data_table.get_item_mut(table_ptr).unwrap();
        &mut data_page[index as usize]
    }

    /// The given [`TablePtr`] need to be unique.
    fn write_impl(
        data_table: &mut Table<[T; PAGE_SIZE]>,
        page_tables: &mut [Table<[TablePtr; PAGE_SIZE]>],
        table_ptr: &mut TablePtr,
        index: u64,
        divisor: u64,
        buf: &[T],
    ) {
        let first_page = index / divisor;
        let last_page = (index + buf.len() as u64).div_ceil(divisor);

        let page_index = index - (first_page * divisor);

        let amount_of_pages = last_page - first_page;

        if let Some((table, next_page_tables)) = page_tables.split_first_mut() {
            if !table.is_unique_table_ptr(table_ptr) {
                // IDEA: Not using the box is possible by doing the clone inside the match, generating slightly better
                // optimized code. At the cost of readability.

                // Look for next item in the iterator to find the table to clone the ptr's in.
                // If this is the last table, we have to clone inside the data_table.
                let cloner: Box<dyn FnMut(&TablePtr) -> TablePtr> =
                    match next_page_tables.first_mut() {
                        Some(next_table) => Box::new(|ptr| next_table.clone_table_ptr(ptr)),
                        None => Box::new(|ptr| data_table.clone_table_ptr(ptr)),
                    };
                // TablePtr clone is save as `clone_item` will drop the ptr given and we overwrite the
                // original here immediately.
                *table_ptr = table.clone_array_item(table_ptr.unsafe_clone(), cloner);
            }

            let tables = table.get_item_mut(table_ptr).unwrap();

            for i in 0..amount_of_pages {
                let start = (i * divisor).saturating_sub(page_index);

                let end = (i + 1) * divisor - page_index;
                let end = end.min(buf.len() as u64);

                let index = if i == 0 { page_index } else { 0 };

                let table_ptr = &mut tables[(first_page + i) as usize];

                let buf = &buf[(start as usize)..(end as usize)];

                Self::write_impl(
                    data_table,
                    next_page_tables,
                    table_ptr,
                    index,
                    divisor / PAGE_SIZE as u64,
                    buf,
                );
            }
        } else {
            debug_assert!(buf.len() <= PAGE_SIZE - index as usize);
            debug_assert!(divisor == 1);
            let index = index as usize;

            if !data_table.is_unique_table_ptr(table_ptr) {
                *table_ptr = data_table.clone_array_item(table_ptr.unsafe_clone(), T::clone);
            }

            let page = data_table.get_item_mut(table_ptr).unwrap();
            let page = &mut page[(index)..(index + buf.len())];

            page.clone_from_slice(buf);
        }
    }

    fn write(&mut self, table_ptr: &mut TablePtr, index: u64, buf: &[T]) {
        let divisor = self.items_per_top_level_page() / PAGE_SIZE as u64;

        Self::write_impl(
            &mut self.data_table,
            &mut self.page_tables,
            table_ptr,
            index,
            divisor,
            buf,
        );
    }

    /// Clone a top level PagePtr adding an extra reference to the page
    fn clone_table_ptr(&mut self, table_ptr: &TablePtr) -> TablePtr {
        match self.page_tables.first_mut() {
            Some(page_table) => page_table.clone_table_ptr(table_ptr),
            None => self.data_table.clone_table_ptr(table_ptr),
        }
    }

    /// Drop a top level PagePtr removing a reference to the page.
    /// If this was the last reference. The page will be deleted.
    fn drop_table_ptr(&mut self, table_ptr: TablePtr) {
        let mut page_table_iter = self.page_tables.iter_mut();
        match page_table_iter.next() {
            Some(page_table) => {
                let Some(droped) = page_table.pop_item(table_ptr) else {
                    return;
                };

                // Go over the next tables marking all the pages that where stored in the just
                // droped page as also droped.
                let mut droped = vec![droped];
                for table in page_table_iter {
                    let mut next_droped = Vec::new();

                    for droped_ptr in droped.drain(..).flatten() {
                        if let Some(droped) = table.pop_item(droped_ptr) {
                            next_droped.push(droped);
                        }
                    }
                    droped = next_droped;
                    if droped.is_empty() {
                        return;
                    }
                }

                // Any pages the last table droped have ptr's to the data_table.
                for droped_ptr in droped.drain(..).flatten() {
                    self.data_table.pop_item(droped_ptr);
                }
            }
            None => {
                self.data_table.pop_item(table_ptr);
            }
        }
    }
}

impl std::fmt::Debug for (dyn ArrayStorageTrait + 'static) {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("dyn ArrayStorageTrait").finish()
    }
}

impl<T: Copy> ArrayStorageTrait for ArrayStorage<T> {
    fn clone_instance(&mut self, instance: &Instance) -> Instance {
        let pages = instance
            .pages
            .iter()
            .map(|page_ptr| self.clone_table_ptr(page_ptr))
            .collect();
        Instance {
            pages,
            size: instance.size,
            reset_page: self.clone_table_ptr(&instance.reset_page),
        }
    }

    fn drop_instance(&mut self, instance: Instance) {
        for table_ptr in instance.pages.into_vec().into_iter() {
            self.drop_table_ptr(table_ptr);
        }
    }
}

#[derive(Debug)]
pub(crate) struct Instance {
    reset_page: TablePtr,
    pages: Box<[TablePtr]>,
    size: u64,
}

impl Instance {
    pub(crate) fn len(&self) -> u64 {
        self.size
    }

    #[track_caller]
    pub(crate) fn get<'a, T: Copy>(
        &self,
        array_storage: &'a ArrayStorage<T>,
        index: u64,
    ) -> Option<&'a T> {
        if index >= self.len() {
            return None;
        }
        let page = index / array_storage.items_per_top_level_page();
        let index = index - (page * array_storage.items_per_top_level_page());
        Some(array_storage.get(&self.pages[page as usize], index))
    }

    #[track_caller]
    pub(crate) fn get_mut<'a, T: Copy>(
        &mut self,
        array_storage: &'a mut ArrayStorage<T>,
        index: u64,
    ) -> Option<&'a mut T> {
        if index >= self.len() {
            return None;
        }
        let page = index / array_storage.items_per_top_level_page();
        let index = index - (page * array_storage.items_per_top_level_page());
        Some(array_storage.get_mut(&mut self.pages[page as usize], index))
    }

    pub(crate) fn read<T: Copy + 'static>(
        &self,
        array_storage: &ArrayStorage<T>,
        buf: &mut [T],
        index: u64,
    ) -> bool {
        if index as u128 + buf.len() as u128 > self.len() as u128 {
            return false;
        }

        let items_per_top_level_page = array_storage.items_per_top_level_page();

        let first_page = index / items_per_top_level_page;
        let last_page = (index + buf.len() as u64).div_ceil(items_per_top_level_page);
        let total_pages = last_page - first_page;

        let page_index = index - (first_page * items_per_top_level_page);

        for i in 0..total_pages {
            let start = (i * items_per_top_level_page).saturating_sub(page_index);

            let end = (i + 1) * items_per_top_level_page - page_index;
            let end = end.min(buf.len() as u64);

            let buf = &mut buf[(start as usize)..(end as usize)];

            let table_ptr = &self.pages[(first_page + i) as usize];

            let index = if i == 0 { page_index } else { 0 };

            array_storage.read(buf, table_ptr, index);
        }

        true
    }

    pub(crate) fn write<T: Copy + 'static>(
        &mut self,
        array_storage: &mut ArrayStorage<T>,
        index: u64,
        buf: &[T],
    ) -> bool {
        if index as u128 + buf.len() as u128 > self.len() as u128 {
            return false;
        }

        let items_per_top_level_page = array_storage.items_per_top_level_page();

        let first_page = index / items_per_top_level_page;
        let last_page = (index + buf.len() as u64).div_ceil(items_per_top_level_page);
        let total_pages = last_page - first_page;

        let page_index = index - (first_page * items_per_top_level_page);

        for i in 0..total_pages {
            let start = (i * items_per_top_level_page).saturating_sub(page_index);

            let end = (i + 1) * items_per_top_level_page - page_index;
            let end = end.min(buf.len() as u64);

            let buf = &buf[(start as usize)..(end as usize)];

            let table_ptr = &mut self.pages[(first_page + i) as usize];

            let index = if i == 0 { page_index } else { 0 };

            array_storage.write(table_ptr, index, buf);
        }

        true
    }

    pub(crate) fn iter_range<'a, T: Copy + 'static>(
        &'a self,
        array_storage: &'a ArrayStorage<T>,
        index: u64,
        len: u64,
    ) -> Option<impl Iterator<Item = &'a T> + 'a> {
        if index as u128 + len as u128 > self.len() as u128 {
            return None;
        }

        let items_per_top_level_page = array_storage.items_per_top_level_page();

        let first_page = index / items_per_top_level_page;
        let last_page = (index + len).div_ceil(items_per_top_level_page);
        let total_pages = last_page - first_page;

        let page_index = index - (first_page * items_per_top_level_page);

        let iter = (0..total_pages).flat_map(move |i| {
            let table_ptr = &self.pages[(first_page + i) as usize];

            let index = if i == 0 { page_index } else { 0 };

            let len_left = len - (i * items_per_top_level_page).saturating_sub(page_index);

            array_storage.iter_range(
                table_ptr,
                index,
                (items_per_top_level_page - index).min(len_left),
            )
        });

        Some(iter)
    }

    pub(crate) fn reset<T: Copy + 'static>(&mut self, array_storage: &mut ArrayStorage<T>) {
        for page in self.pages.iter_mut() {
            if page != &self.reset_page {
                array_storage.drop_table_ptr(page.unsafe_clone());
                *page = array_storage.clone_table_ptr(&self.reset_page);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_VALUES: [(u64, u32); 18] = [
        (12345, 430125),
        (117877, 260963),
        (172345, 229703),
        (224967, 251133),
        (29821, 46949),
        (35000, 63354),
        (56611, 83512),
        (65913, 96398),
        (73350, 180230),
        (75423, 162558),
        (7, 7),
        (6, 6),
        (5, 5),
        (4, 4),
        (3, 3),
        (2, 2),
        (1, 1),
        (0, 0),
    ];

    const KINDA_BIG_PRIME: u64 = 65537;

    #[test]
    fn instance_new() {
        let mut array_storage = ArrayStorage::<u32>::new(3);
        let instance = array_storage.new_instance(0, (u32::MAX / 4) as u64);

        for i in 0..(u32::MAX as u64 / 4 / KINDA_BIG_PRIME) {
            assert_eq!(instance.get(&array_storage, i * KINDA_BIG_PRIME), Some(&0));
        }
    }

    #[test]
    fn instance_storage() {
        let mut array_storage = ArrayStorage::<u32>::new(3);
        let mut instance = array_storage.new_instance(0, (u32::MAX / 4) as u64);

        for j in 0..32 {
            for (i, v) in &TEST_VALUES {
                *instance
                    .get_mut(&mut array_storage, *i + j * KINDA_BIG_PRIME)
                    .unwrap() = *v;
            }
        }

        for j in 0..32 {
            for (i, v) in &TEST_VALUES {
                assert_eq!(
                    instance.get(&array_storage, *i + j * KINDA_BIG_PRIME),
                    Some(v)
                );
            }
        }
    }

    #[test]
    fn instance_reset() {
        let mut array_storage = ArrayStorage::<u32>::new(3);
        let mut instance = array_storage.new_instance(0, (u32::MAX / 4) as u64);

        for j in 0..32 {
            for (i, v) in &TEST_VALUES {
                *instance
                    .get_mut(&mut array_storage, *i + j * KINDA_BIG_PRIME)
                    .unwrap() = *v;
            }
        }

        for j in 0..32 {
            for (i, v) in &TEST_VALUES {
                assert_eq!(
                    instance.get(&array_storage, *i + j * KINDA_BIG_PRIME),
                    Some(v)
                );
            }
        }

        instance.reset(&mut array_storage);

        for j in 0..32 {
            for (i, _) in &TEST_VALUES {
                assert_eq!(
                    instance.get(&array_storage, *i + j * KINDA_BIG_PRIME),
                    Some(&0)
                );
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn array_storage_new() {
        let mut array_storage = ArrayStorage::<u32>::new(2);
        let page = array_storage.add_top_level_page(0);

        for i in 0..array_storage.items_per_top_level_page() {
            assert_eq!(*array_storage.get(&page, i), 0);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn array_storage_write() {
        let mut array_storage = ArrayStorage::<u32>::new(2);
        let mut page = array_storage.add_top_level_page(0);

        for i in 0..array_storage.items_per_top_level_page() {
            assert_eq!(*array_storage.get(&page, i), 0);
        }

        for (i, v) in &TEST_VALUES {
            *array_storage.get_mut(&mut page, *i) = *v;
        }

        for (i, v) in &TEST_VALUES {
            assert_eq!(*array_storage.get(&page, *i), *v);
        }
    }

    #[test]
    fn array_storage_new_zero_layers() {
        let mut array_storage = ArrayStorage::<u32>::new(0);
        let page = array_storage.add_top_level_page(128);

        for i in 0..array_storage.items_per_top_level_page() {
            assert_eq!(*array_storage.get(&page, i), 128);
        }
    }

    #[test]
    fn array_storage_write_zero_layer() {
        let mut array_storage = ArrayStorage::<u32>::new(0);
        let mut page = array_storage.add_top_level_page(0);

        for i in 0..array_storage.items_per_top_level_page() {
            assert_eq!(*array_storage.get(&page, i), 0);
        }

        let test_values = [
            (7, 7),
            (6, 6),
            (5, 5),
            (4, 4),
            (3, 3),
            (2, 2),
            (1, 1),
            (0, 0),
        ];

        for (i, v) in &test_values {
            *array_storage.get_mut(&mut page, *i) = *v;
        }

        for (i, v) in &test_values {
            assert_eq!(*array_storage.get(&page, *i), *v);
        }
    }

    #[test]
    fn buf_read_and_write() {
        let mut array_storage = ArrayStorage::<u32>::new(1);
        let mut instance =
            array_storage.new_instance(0, array_storage.items_per_top_level_page() * 2);

        let mut buf_write = vec![0; (instance.len() - 4) as usize];
        let mut i: u32 = 0;
        buf_write.fill_with(|| {
            i = i.wrapping_add(KINDA_BIG_PRIME as u32);
            i
        });

        assert!(instance.write(&mut array_storage, 4, &buf_write));

        let mut buf_read = vec![0; (instance.len() - 2) as usize];
        assert!(instance.read(&array_storage, &mut buf_read, 2));

        assert_eq!(&buf_read[..2], &[0, 0]);
        assert_eq!(&buf_read[2..], &buf_write);
    }

    #[test]
    fn buf_read_and_write_sec_page() {
        let mut array_storage = ArrayStorage::<u32>::new(1);
        let size = array_storage.items_per_top_level_page();

        let mut instance = array_storage.new_instance(0, size * 2);

        let mut buf_write = vec![0; (size - 4) as usize];
        let mut i: u32 = 0;
        buf_write.fill_with(|| {
            i = i.wrapping_add(KINDA_BIG_PRIME as u32);
            i + 1
        });
        assert!(instance.write(&mut array_storage, size + 4, &buf_write));

        let mut buf_read = vec![0; (size - 2) as usize];
        assert!(instance.read(&array_storage, &mut buf_read, size + 2));

        assert_eq!(&buf_read[..2], &[0, 0]);
        assert_eq!(&buf_read[2..], &buf_write);
    }

    #[test]
    fn buf_read_and_write_over_page_boundary() {
        let mut array_storage = ArrayStorage::<u32>::new(1);
        let size = array_storage.items_per_top_level_page();

        let mut instance = array_storage.new_instance(0, size * 2);

        let mut buf_write = vec![0; (size - 4) as usize];
        let mut i: u32 = 0;
        buf_write.fill_with(|| {
            i = i.wrapping_add(KINDA_BIG_PRIME as u32);
            i + 1
        });
        assert!(instance.write(&mut array_storage, size / 2 + 4, &buf_write));

        let mut buf_read = vec![0; (size - 2) as usize];
        assert!(instance.read(&array_storage, &mut buf_read, size / 2 + 2));

        assert_eq!(&buf_read[..2], &[0, 0]);
        assert_eq!(&buf_read[2..], &buf_write);
    }

    #[test]
    fn iterator() {
        let mut array_storage = ArrayStorage::<u32>::new(1);
        let size = array_storage.items_per_top_level_page();

        let mut instance = array_storage.new_instance(0, size * 2);

        let mut buf_write = vec![0; (size * 2 - 4) as usize];
        let mut i: u32 = 0;
        buf_write.fill_with(|| {
            i = i.wrapping_add(KINDA_BIG_PRIME as u32);
            i
        });
        assert!(instance.write(&mut array_storage, 2, &buf_write));

        let mut i: u32 = 0;

        assert!(
            instance
                .iter_range(&array_storage, 0, 2)
                .unwrap()
                .all(|v| *v == 0),
            "Start with 2 zeros"
        );
        assert!(
            instance
                .iter_range(&array_storage, size * 2 - 2, 2)
                .unwrap()
                .all(|v| *v == 0),
            "End with 2 zeros"
        );

        for from_array_storage in instance
            .iter_range(&array_storage, 2, size * 2 - 4)
            .unwrap()
        {
            i = i.wrapping_add(KINDA_BIG_PRIME as u32);

            assert_eq!(from_array_storage, &i);
        }
    }

    #[test]
    fn creating_and_removing_instance() {
        let mut array_storage = ArrayStorage::<u32>::new(1);
        let size = array_storage.items_per_top_level_page();

        let mut instances = Vec::new();
        for i in 1..5 {
            let mut instance = array_storage.new_instance(i, size * i as u64);

            let mut j = 0;
            while j < instance.len() {
                let v = instance.get_mut(&mut array_storage, j).unwrap();
                *v = v.wrapping_mul(2);
                j += 101;
            }

            let instance2 = array_storage.new_instance(101, size);
            instances.push(instance2);

            array_storage.remove_instance(instance);
        }

        for instance in instances {
            for v in instance.iter_range(&array_storage, 0, size).unwrap() {
                assert_eq!(v, &101);
            }
        }
    }

    #[test]
    fn string_read_write() {
        let mut array_storage = ArrayStorage::<u8>::new(3);
        let size = array_storage.items_per_top_level_page();

        let mut instance = array_storage.new_instance(0, size * 2);

        let buf = b"Hello, world\n\0Type a character: ";

        assert!(instance.write(&mut array_storage, 64 - 19, buf));

        let mut buf_read = vec![0u8; 32];
        assert!(instance.read(&array_storage, &mut buf_read, 64 - 19));

        assert_eq!(&buf_read, &buf);
    }
}
