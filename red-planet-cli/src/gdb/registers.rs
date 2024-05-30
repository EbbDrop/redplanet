use gdbstub::target::{
    ext::base::single_register_access::SingleRegisterAccess, TargetError, TargetResult,
};
use gdbstub_arch::riscv::reg::id::RiscvRegId;
use red_planet_core::registers::Specifier;
use std::io::Write;

use super::SimTarget;

impl SingleRegisterAccess<()> for SimTarget {
    fn read_register(
        &mut self,
        _tid: (),
        reg_id: RiscvRegId<u32>,
        mut buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        let (allocator, board) = self.simulator.inspect();

        match reg_id {
            RiscvRegId::Gpr(i) => {
                let registers = board.core().registers(allocator);
                let value = registers.x(Specifier::new(i).unwrap());
                let bytes = value.to_le_bytes(); // TODO: this should be the "native byte order"
                Ok(buf.write(&bytes)?)
            }
            RiscvRegId::Fpr(_) => todo!(),
            RiscvRegId::Pc => {
                let value = board.core().registers(allocator).pc();
                let bytes = value.to_le_bytes(); // TODO: this should be the "native byte order"
                Ok(buf.write(&bytes)?)
            }
            RiscvRegId::Csr(specifier) => {
                let result = self
                    .simulator
                    .step_with("inspect csr", move |allocator, board| {
                        board.core().read_csr(
                            allocator,
                            specifier,
                            board.core().privilege_mode(allocator),
                        )
                    });
                match result {
                    Ok(value) => {
                        let bytes = value.to_le_bytes();
                        Ok(buf.write(&bytes)?)
                    }
                    Err(_) => Err(TargetError::NonFatal),
                }
            }
            RiscvRegId::Priv => match buf.first_mut() {
                Some(byte) => {
                    *byte = board.core().privilege_mode(allocator) as u8;
                    Ok(1)
                }
                None => Ok(0),
            },
            _ => Err(TargetError::NonFatal),
        }
    }

    fn write_register(
        &mut self,
        _tid: (),
        reg_id: RiscvRegId<u32>,
        val: &[u8],
    ) -> TargetResult<(), Self> {
        let val = val.to_owned();
        self.simulator.step_with(
            "gdb write single register",
            move |allocator, board| match reg_id {
                RiscvRegId::Gpr(i) => {
                    let mut buf = [0u8; 4];
                    buf.as_mut_slice().write_all(&val)?;
                    let registers = board.core().registers_mut(allocator);
                    // TODO: this should be the "native byte order"
                    registers.set_x(Specifier::new(i).unwrap(), u32::from_le_bytes(buf));
                    Ok(())
                }
                RiscvRegId::Fpr(_) => todo!(),
                RiscvRegId::Pc => {
                    let mut buf = [0u8; 4];
                    buf.as_mut_slice().write_all(&val)?;
                    let registers = board.core().registers_mut(allocator);
                    // TODO: this should be the "native byte order"
                    *registers.pc_mut() = u32::from_le_bytes(buf);
                    Ok(())
                }
                RiscvRegId::Csr(specifier) => {
                    let mut buf = [0u8; 4];
                    buf.as_mut_slice().write_all(&val)?;
                    board
                        .core()
                        .write_csr(
                            allocator,
                            specifier,
                            board.core().privilege_mode(allocator),
                            u32::from_le_bytes(buf),
                            0xFFFF_FFFF,
                        )
                        .map_err(|_| TargetError::NonFatal)
                }
                RiscvRegId::Priv => todo!(),
                _ => Err(TargetError::NonFatal),
            },
        )
    }
}
