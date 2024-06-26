use gdbstub::{
    arch::Arch,
    target::{
        ext::breakpoints::{Breakpoints, HwBreakpoint, SwBreakpoint, SwBreakpointOps},
        TargetResult,
    },
};

use crate::{gdb::GdbTarget, target::command::Command};

impl Breakpoints for GdbTarget {
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

impl SwBreakpoint for GdbTarget {
    fn add_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.send_command(Command::AddBreakpoint(addr))?;
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.send_command(Command::RemoveBreakpoint(addr))?;
        Ok(true)
    }
}

impl HwBreakpoint for GdbTarget {
    fn add_hw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.send_command(Command::AddBreakpoint(addr))?;
        Ok(true)
    }

    fn remove_hw_breakpoint(
        &mut self,
        addr: u32,
        _kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.send_command(Command::RemoveBreakpoint(addr))?;
        Ok(true)
    }
}
