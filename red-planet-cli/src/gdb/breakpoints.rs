use gdbstub::{
    arch::Arch,
    target::{
        ext::breakpoints::{Breakpoints, HwBreakpoint, SwBreakpoint, SwBreakpointOps},
        TargetResult,
    },
};

use crate::target::SimTarget;

impl Breakpoints for SimTarget {
    fn support_sw_breakpoint(&mut self) -> Option<SwBreakpointOps<'_, Self>> {
        Some(self)
    }

    fn support_hw_breakpoint(
        &mut self,
    ) -> Option<gdbstub::target::ext::breakpoints::HwBreakpointOps<'_, Self>> {
        Some(self)
    }

    fn support_hw_watchpoint(
        &mut self,
    ) -> Option<gdbstub::target::ext::breakpoints::HwWatchpointOps<'_, Self>> {
        None
    }
}

impl SwBreakpoint for SimTarget {
    fn add_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.breakpoints.insert(addr);
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.breakpoints.remove(&addr);
        Ok(true)
    }
}

impl HwBreakpoint for SimTarget {
    fn add_hw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.breakpoints.insert(addr);
        Ok(true)
    }

    fn remove_hw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.breakpoints.remove(&addr);
        Ok(true)
    }
}
