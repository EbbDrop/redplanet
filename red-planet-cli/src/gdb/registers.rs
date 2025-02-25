use gdbstub::target::{
    ext::base::single_register_access::SingleRegisterAccess, TargetError, TargetResult,
};
use gdbstub_arch::riscv::reg::id::RiscvRegId;
use std::io::Write;

use crate::{gdb::GdbTarget, target::command::Command};

use super::GdbTargetError;

impl SingleRegisterAccess<()> for GdbTarget {
    fn read_register(
        &mut self,
        _tid: (),
        reg_id: RiscvRegId<u32>,
        mut buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        let (sender, reciver) = oneshot::channel();
        self.send_command(Command::ReadRegister(reg_id, sender))?;
        let value = reciver.recv().map_err(|_| TargetError::NonFatal)?;
        let bytes = value.to_le_bytes();
        buf.write_all(&bytes)?;
        Ok(bytes.len())
    }

    fn write_register(
        &mut self,
        _tid: (),
        reg_id: RiscvRegId<u32>,
        val: &[u8],
    ) -> TargetResult<(), Self> {
        let (sender, reciver) = oneshot::channel();
        self.send_command(Command::WriteRegister(reg_id, val.to_owned(), sender))?;

        reciver
            .recv()
            .map_err(|_| TargetError::Fatal(GdbTargetError::NoAnswer))?
            .map_err(|_| TargetError::NonFatal)
    }
}
