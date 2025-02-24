mod base_ops;
mod breakpoints;
mod registers;
mod resume;
mod step;

use gdbstub::{
    arch::Arch,
    common::Signal,
    conn::Connection,
    stub::{
        state_machine::GdbStubStateMachine, DisconnectReason, GdbStub, GdbStubError,
        SingleThreadStopReason,
    },
    target::{
        ext::{
            base::{reverse_exec::ReplayLogPosition, BaseOps},
            breakpoints::BreakpointsOps,
        },
        Target, TargetError,
    },
};
use gdbstub_arch::riscv::{
    reg::{id::RiscvRegId, RiscvCoreRegs},
    Riscv32,
};
use tokio::{
    io::AsyncReadExt,
    select,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};

use crate::{
    target::{command::Command, Event},
    tcp::TcpStream,
};

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

#[derive(Debug)]
pub enum GdbTargetError {
    TargetGone,
    NoAnswer,
}

impl std::fmt::Display for GdbTargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unknown target error")
    }
}

pub struct GdbTarget {
    command_sender: UnboundedSender<Command>,
    event_receiver: UnboundedReceiver<Event>,
}

impl GdbTarget {
    pub fn new(
        command_sender: UnboundedSender<Command>,
        event_receiver: UnboundedReceiver<Event>,
    ) -> Self {
        Self {
            command_sender,
            event_receiver,
        }
    }

    pub fn send_command(
        &mut self,
        command: Command,
    ) -> Result<(), TargetError<<Self as Target>::Error>> {
        self.command_sender
            .send(command)
            .map_err(|_| TargetError::Fatal(GdbTargetError::TargetGone))
    }
}

impl Target for GdbTarget {
    type Arch = OurRiscv32;
    type Error = GdbTargetError;

    fn base_ops(&mut self) -> BaseOps<Self::Arch, Self::Error> {
        // Indicate our target is single-threaded
        BaseOps::SingleThread(self)
    }

    fn support_breakpoints(&mut self) -> Option<BreakpointsOps<'_, Self>> {
        Some(self)
    }
}

#[derive(Debug)]
pub enum GdbError {
    Connection(<TcpStream as Connection>::Error),
    Inner(GdbStubError<<GdbTarget as Target>::Error, <TcpStream as Connection>::Error>),
    TargetThreadStoped,
}

pub async fn run_server(
    connection: TcpStream,
    target: &mut GdbTarget,
) -> Result<DisconnectReason, GdbError> {
    let debugger = GdbStub::new(connection);

    let mut gdb = debugger
        .run_state_machine(target)
        .map_err(GdbError::Inner)?;
    loop {
        gdb = match gdb {
            GdbStubStateMachine::Idle(mut gdb) => {
                // needs more data, so perform a blocking read on the connection
                let byte = gdb
                    .borrow_conn()
                    .0
                    .read_u8()
                    .await
                    .map_err(GdbError::Connection)?;
                gdb.incoming_data(target, byte).map_err(GdbError::Inner)?
            }

            GdbStubStateMachine::Disconnected(gdb) => {
                break Ok(gdb.get_reason());
            }

            GdbStubStateMachine::CtrlCInterrupt(gdb) => {
                let _ = target.command_sender.send(Command::Pause);
                gdb.interrupt_handled(target, None::<SingleThreadStopReason<u32>>)
                    .map_err(GdbError::Inner)?
            }

            GdbStubStateMachine::Running(mut gdb) => {
                // block waiting for the target to return a stop reason
                let conn = gdb.borrow_conn();
                select! {
                    byte = conn.0.read_u8() => {
                        match byte {
                            Ok(byte) => gdb.incoming_data(target, byte).map_err(GdbError::Inner)?,
                            Err(error) => return Err(GdbError::Connection(error)),
                        }
                    }
                    event = target.event_receiver.recv() => {
                        let event = event.ok_or(GdbError::TargetThreadStoped)?;
                        let stop_reason = match event {
                            Event::DoneStep => SingleThreadStopReason::DoneStep,
                            Event::ReachedStart => SingleThreadStopReason::ReplayLog {
                                tid: None,
                                pos: ReplayLogPosition::Begin,
                            },
                            Event::PoweredDown => SingleThreadStopReason::DoneStep,
                            Event::Break => SingleThreadStopReason::SwBreak(()),
                            Event::Pause => SingleThreadStopReason::Signal(Signal::SIGINT),
                        };
                        gdb.report_stop(target, stop_reason).map_err(GdbError::Inner)?
                    }
                }
            }
        }
    }
}
