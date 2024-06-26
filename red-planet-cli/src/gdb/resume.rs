use gdbstub::{
    common::Signal,
    target::ext::base::{
        reverse_exec::{ReverseCont, ReverseContOps, ReverseStepOps},
        singlethread::{
            SingleThreadRangeSteppingOps, SingleThreadResume, SingleThreadSingleStepOps,
        },
    },
};

use crate::target::command::Command;

use super::{GdbTarget, GdbTargetError};

impl SingleThreadResume for GdbTarget {
    fn resume(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        self.send_command(Command::Continue)
            .map_err(|_| GdbTargetError::TargetGone)
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

impl ReverseCont<()> for GdbTarget {
    fn reverse_cont(&mut self) -> Result<(), Self::Error> {
        self.send_command(Command::ReverseContinue)
            .map_err(|_| GdbTargetError::TargetGone)
    }
}
