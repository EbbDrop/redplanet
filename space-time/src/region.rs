use std::ops::{Index, IndexMut};

use crate::SpaceTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionHandle {}

pub struct Region<'a> {
    pub(crate) space_time: &'a SpaceTime,
}

pub struct RegionMut<'a> {
    pub(crate) space_time: &'a mut SpaceTime,
}

impl<'a> Index<u32> for Region<'a> {
    type Output = u32;

    fn index(&self, index: u32) -> &Self::Output {
        &self.get_ref(index).expect("TODO")
    }
}

impl<'a> Region<'a> {
    pub fn get(&self, index: u32) -> Option<u32> {
        todo!()
    }

    pub fn get_ref(&self, index: u32) -> Option<&u32> {
        todo!()
    }
}

impl<'a> Index<u32> for RegionMut<'a> {
    type Output = u32;

    fn index(&self, index: u32) -> &Self::Output {
        self.get_ref(index).as_ref().expect("TODO")
    }
}

impl<'a> IndexMut<u32> for RegionMut<'a> {
    fn index_mut(&mut self, index: u32) -> &mut Self::Output {
        self.get_mut(index).expect("TODO")
    }
}

impl<'a> RegionMut<'a> {
    pub fn get(&self, index: u32) -> Option<u32> {
        todo!()
    }

    pub fn get_ref(&self, index: u32) -> Option<&u32> {
        todo!()
    }

    pub fn get_mut(&mut self, index: u32) -> Option<&mut u32> {
        todo!()
    }

    /// Writes a single value into this region.
    ///
    /// Returns the old value. When [`None`] is returned nothing is written
    /// because you tried to write outside the region.
    pub fn write(&mut self, index: u32, val: u32) -> Option<u32> {
        todo!()
    }
}
