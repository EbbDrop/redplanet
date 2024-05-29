use std::ops::Deref;

use space_time::allocator::Allocator;

pub trait IrqCallback<A: Allocator> {
    fn raise(&self, allocator: &mut A);

    fn lower(&self, allocator: &mut A);
}

pub struct DynIrqCallback<A: Allocator>(pub Box<dyn IrqCallback<A>>);

impl<A: Allocator> Deref for DynIrqCallback<A> {
    type Target = dyn IrqCallback<A>;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl<A: Allocator> std::fmt::Debug for DynIrqCallback<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynIrqCallback").finish_non_exhaustive()
    }
}
