use gdbstub::target::{
    ext::base::{
        single_register_access::SingleRegisterAccessOps,
        singlethread::{SingleThreadBase, SingleThreadResumeOps},
    },
    TargetError, TargetResult,
};
use gdbstub_arch::riscv::reg::RiscvCoreRegs;
use red_planet_core::registers::Specifier;

use crate::target::SimTarget;

impl SingleThreadBase for SimTarget {
    fn read_registers(&mut self, regs: &mut RiscvCoreRegs<u32>) -> TargetResult<(), Self> {
        let (allocator, board) = self.simulator.inspect();

        let registers = board.core().registers(allocator);

        for r in Specifier::iter_all() {
            regs.x[usize::from(r)] = registers.x(r);
        }

        regs.pc = registers.pc();

        Ok(())
    }

    fn write_registers(&mut self, regs: &RiscvCoreRegs<u32>) -> TargetResult<(), Self> {
        let regs = regs.clone();
        self.simulator
            .step_with("gdb write all registers", move |allocator, board| {
                let registers = board.core().registers_mut(allocator);
                for r in Specifier::iter_all() {
                    registers.set_x(r, regs.x[usize::from(r)])
                }
                *registers.pc_mut() = regs.pc;
            });

        Ok(())
    }

    fn support_single_register_access(&mut self) -> Option<SingleRegisterAccessOps<'_, (), Self>> {
        Some(self)
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<usize, Self> {
        let (allocator, board) = self.simulator.inspect();

        let memory = board.core().mmu();

        match memory.read_range_debug(data, allocator, start_addr) {
            Ok(()) => Ok(data.len()),
            Err(_) => Err(TargetError::NonFatal),
        }
    }

    fn write_addrs(&mut self, start_addr: u32, data: &[u8]) -> TargetResult<(), Self> {
        let data = data.to_owned();
        let write_res = self
            .simulator
            .step_with("gdb write memory", move |allocator, board| {
                let memory = board.core().mmu();
                memory.write_range(allocator, start_addr, &data)
            });

        write_res.map_err(|_| TargetError::NonFatal)
    }

    fn support_resume(&mut self) -> Option<SingleThreadResumeOps<'_, Self>> {
        Some(self)
    }
}
