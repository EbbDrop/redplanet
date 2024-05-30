use gdbstub::{
    common::Signal,
    target::ext::base::{
        reverse_exec::ReverseStep,
        singlethread::{SingleThreadRangeStepping, SingleThreadSingleStep},
    },
};
use log::info;

use crate::target::ExecutionMode;

use super::SimTarget;

impl SingleThreadSingleStep for SimTarget {
    fn step(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        info!("single stepping");
        self.execution_mode = ExecutionMode::Step;
        Ok(())
    }
}

impl ReverseStep<()> for SimTarget {
    fn reverse_step(&mut self, _tid: ()) -> Result<(), Self::Error> {
        info!("reverse stepping");
        self.execution_mode = ExecutionMode::StepBack;
        Ok(())
    }
}

impl SingleThreadRangeStepping for SimTarget {
    fn resume_range_step(&mut self, start: u32, end: u32) -> Result<(), Self::Error> {
        info!("range stepping");
        self.execution_mode = ExecutionMode::RangeStep(start, end);
        Ok(())
    }
}
