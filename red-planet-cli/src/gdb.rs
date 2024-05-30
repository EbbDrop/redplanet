mod base_ops;
mod breakpoints;
mod registers;
mod resume;
mod step;

use gdbstub::{
    arch::Arch,
    target::{
        ext::{base::BaseOps, breakpoints::BreakpointsOps},
        Target,
    },
};
use gdbstub_arch::riscv::{
    reg::{id::RiscvRegId, RiscvCoreRegs},
    Riscv32,
};

use super::SimTarget;

pub struct OurRiscv32;

impl Arch for OurRiscv32 {
    type Usize = u32;
    type Registers = RiscvCoreRegs<u32>;
    type BreakpointKind = <Riscv32 as Arch>::BreakpointKind;
    type RegId = RiscvRegId<u32>;

    fn target_description_xml() -> Option<&'static str> {
        Some(include_str!("./gdb/rv32-csrs.xml"))
    }
}

impl Target for SimTarget {
    type Arch = OurRiscv32;
    type Error = ();

    fn base_ops(&mut self) -> BaseOps<Self::Arch, Self::Error> {
        // Indicate our target is single-threaded
        BaseOps::SingleThread(self)
    }

    fn support_breakpoints(&mut self) -> Option<BreakpointsOps<'_, Self>> {
        Some(self)
    }
}
