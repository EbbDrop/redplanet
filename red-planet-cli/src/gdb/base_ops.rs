use gdbstub::target::{
    ext::base::{
        single_register_access::SingleRegisterAccessOps,
        singlethread::{SingleThreadBase, SingleThreadResumeOps},
    },
    TargetError, TargetResult,
};
use gdbstub_arch::riscv::reg::RiscvCoreRegs;
use red_planet_core::registers::{Registers, Specifier};

use crate::{
    gdb::{GdbTarget, GdbTargetError},
    target::command::Command,
};

impl SingleThreadBase for GdbTarget {
    fn support_resume(&mut self) -> Option<SingleThreadResumeOps<'_, Self>> {
        Some(self)
    }

    fn read_registers(&mut self, regs: &mut RiscvCoreRegs<u32>) -> TargetResult<(), Self> {
        let (sender, reciver) = oneshot::channel();
        self.send_command(Command::ReadRegisters(sender))?;
        let registers = reciver
            .recv()
            .map_err(|_| TargetError::Fatal(GdbTargetError::NoAnswer))?;

        for r in red_planet_core::registers::Specifier::iter_all() {
            regs.x[usize::from(r)] = registers.x(r);
        }
        regs.pc = registers.pc();
        Ok(())
    }

    fn write_registers(&mut self, regs: &RiscvCoreRegs<u32>) -> TargetResult<(), Self> {
        let mut registers = Registers::default();
        for r in Specifier::iter_all() {
            registers.set_x(r, regs.x[usize::from(r)])
        }
        *registers.pc_mut() = regs.pc;

        self.send_command(Command::WriteRegisters(registers))
    }

    fn support_single_register_access(&mut self) -> Option<SingleRegisterAccessOps<'_, (), Self>> {
        Some(self)
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<usize, Self> {
        let (sender, reciver) = oneshot::channel();
        self.send_command(Command::ReadAddrs(start_addr, data.len(), sender))?;
        let r = reciver
            .recv()
            .map_err(|_| TargetError::Fatal(GdbTargetError::NoAnswer))??;

        data[..r.len()].clone_from_slice(&r);
        Ok(r.len())
    }

    fn write_addrs(&mut self, start_addr: u32, data: &[u8]) -> TargetResult<(), Self> {
        let (sender, reciver) = oneshot::channel();
        self.send_command(Command::WriteAddrs(start_addr, data.to_owned(), sender))?;

        reciver
            .recv()
            .map_err(|_| TargetError::Fatal(GdbTargetError::NoAnswer))?
            .map_err(|_| TargetError::NonFatal)
    }
}
