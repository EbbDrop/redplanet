use gdbstub::{
    common::Signal,
    target::ext::base::{
        reverse_exec::{ReverseCont, ReverseContOps, ReverseStepOps},
        singlethread::{
            SingleThreadRangeSteppingOps, SingleThreadResume, SingleThreadSingleStepOps,
        },
    },
};

use crate::target::ExecutionMode;

use super::SimTarget;

impl SingleThreadResume for SimTarget {
    fn resume(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        self.execution_mode = ExecutionMode::Continue;
        Ok(())
    }

    fn support_single_step(&mut self) -> Option<SingleThreadSingleStepOps<'_, Self>> {
        Some(self)
    }

    fn support_range_step(&mut self) -> Option<SingleThreadRangeSteppingOps<'_, Self>> {
        Some(self)
    }

    fn support_reverse_step(&mut self) -> Option<ReverseStepOps<'_, (), Self>> {
        Some(self)
    }

    fn support_reverse_cont(&mut self) -> Option<ReverseContOps<'_, (), Self>> {
        Some(self)
    }
}

impl ReverseCont<()> for SimTarget {
    fn reverse_cont(&mut self) -> Result<(), Self::Error> {
        self.execution_mode = ExecutionMode::ReverseContinue;
        Ok(())
    }
}
