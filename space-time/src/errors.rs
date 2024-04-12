/// This error indicates an invalid [`crate::Allocator::Id`] or [`crate::Allocator::ArrayId`] was
/// used.
///
/// Within the context of one [`crate::Allocator`], an id can be invalid if it has never been
/// created by that [`crate::Allocator`], or if it has been removed or popped from the
/// [`crate::Allocator`].
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct InvalidIdError;

/// This error indicates an invalid [`crate::SnapshotId`] was used.
///
/// Within the context of one [`crate::SpaceTime`], a [`crate::SnapshotId`] can be invalid if it has
/// never been created by the [`crate::SpaceTime`], or if it has been dropped from the
/// [`crate::SpaceTime`].
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct InvalidSnapshotIdError;
