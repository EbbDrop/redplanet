use gdbstub::{
    common::Signal,
    target::ext::base::{
        reverse_exec::ReverseStep,
        singlethread::{SingleThreadRangeStepping, SingleThreadSingleStep},
    },
};

use crate::target::command::Command;

use super::{GdbTarget, GdbTargetError};

impl SingleThreadSingleStep for GdbTarget {
    fn step(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        self.send_command(Command::Step)
            .map_err(|_| GdbTargetError::TargetGone)
    }
}

impl ReverseStep<()> for GdbTarget {
    fn reverse_step(&mut self, _tid: ()) -> Result<(), Self::Error> {
        self.send_command(Command::StepBack)
            .map_err(|_| GdbTargetError::TargetGone)
    }
}

impl SingleThreadRangeStepping for GdbTarget {
    fn resume_range_step(&mut self, start: u32, end: u32) -> Result<(), Self::Error> {
        self.send_command(Command::RangeStep(start, end))
            .map_err(|_| GdbTargetError::TargetGone)
    }
}
