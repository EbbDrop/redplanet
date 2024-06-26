use gdbstub::target::TargetError;
use gdbstub_arch::riscv::reg::id::RiscvRegId;
use red_planet_core::{core::mmu::MemoryError, registers::Registers};

use crate::gdb::GdbTargetError;

type FailableReturnChannel<T> = oneshot::Sender<Result<T, TargetError<GdbTargetError>>>;

pub enum Command {
    // Close the program
    Exit,
    // Pause execution
    Pause,
    Continue,
    ReverseContinue,
    Step,
    StepBack,
    RangeStep(u32, u32),
    RemoveBreakpoint(u32),
    AddBreakpoint(u32),
    ReadRegisters(oneshot::Sender<Registers>),
    WriteRegisters(Registers),
    ReadAddrs(u32, usize, FailableReturnChannel<Vec<u8>>),
    WriteAddrs(u32, Vec<u8>, oneshot::Sender<Result<(), MemoryError>>),
    DeleteFuture,
    GoTo(usize),
    ReadRegister(RiscvRegId<u32>, oneshot::Sender<u32>),
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::Exit => write!(f, "Exit"),
            Command::Pause => write!(f, "Stop"),
            Command::Continue => write!(f, "Continue"),
            Command::ReverseContinue => write!(f, "ReverseContinue"),
            Command::Step => write!(f, "Step"),
            Command::StepBack => write!(f, "ReverseStep"),
            Command::RangeStep(_, _) => write!(f, "RangeStep"),
            Command::RemoveBreakpoint(_) => write!(f, "RemoveBreakpoint"),
            Command::AddBreakpoint(_) => write!(f, "AddBreakpoint"),
            Command::ReadRegisters(_) => write!(f, "ReadRegisters"),
            Command::ReadRegister(_, _) => write!(f, "ReadRegister"),
            Command::WriteRegisters(_) => write!(f, "WriteRegisters"),
            Command::ReadAddrs(_, _, _) => write!(f, "ReadAddrs"),
            Command::WriteAddrs(_, _, _) => write!(f, "WriteAddrs"),
            Command::DeleteFuture => write!(f, "DeleteFuture"),
            Command::GoTo(_) => write!(f, "GoTo"),
        }
    }
}
