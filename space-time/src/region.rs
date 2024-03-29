use std::ops::{Index, IndexMut};

use crate::{errors::OutOffBounds, SpaceTime};

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
        self.get_ref(index).expect("TODO")
    }
}

impl<'a> Region<'a> {
    pub fn get(&self, index: u32) -> Result<u32, OutOffBounds> {
        todo!()
    }

    pub fn get_ref(&self, index: u32) -> Result<&u32, OutOffBounds> {
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
    pub fn get(&self, index: u32) -> Result<u32, OutOffBounds> {
        todo!()
    }

    pub fn get_ref(&self, index: u32) -> Result<&u32, OutOffBounds> {
        todo!()
    }

    /// Writes to the given reference should be deterministic, if you can't guarantee
    /// this use [`as_non_determinitic_writer`](Self::as_non_determinitic_writer)
    pub fn get_mut(&mut self, index: u32) -> Result<&mut u32, OutOffBounds> {
        todo!()
    }

    /// Writes a single value into this region.
    ///
    /// Writes should be deterministic, if you can't guarantee
    /// this use [`as_non_determinitic_writer`](Self::as_non_determinitic_writer)
    ///
    /// Returns the old value. When [`None`] is returned nothing is written
    /// because you tried to write outside the region.
    pub fn set(&mut self, index: u32, val: u32) -> Result<u32, OutOffBounds> {
        todo!()
    }

    /// `Some` if the [`SpaceTime`] is currently not in the past.
    /// When this function returns `None` the [`SpaceTime`] is in the past.
    /// Use reads from the region to get the value it was back then.
    pub fn as_non_determinitic_writer(&'a mut self) -> Option<NonDeterministicWriter<'a>> {
        // TODO: check if in passed
        Some(NonDeterministicWriter { region: self })
    }
}

pub struct NonDeterministicWriter<'a> {
    region: &'a mut RegionMut<'a>,
}

impl<'a> NonDeterministicWriter<'a> {
    /// Writes a single value into this region.
    ///
    /// When [`Err`] is returned nothing is written
    /// because you tried to write outside the region.
    pub fn set(&mut self, index: u32, val: u32) -> Result<(), OutOffBounds> {
        todo!()
    }
}
