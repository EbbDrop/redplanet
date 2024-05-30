use std::io;
use std::net::{TcpListener, TcpStream};

use gdbstub::common::Signal;
use gdbstub::conn::{Connection, ConnectionExt};
use gdbstub::stub::SingleThreadStopReason;
use gdbstub::stub::{run_blocking, DisconnectReason, GdbStub};
use gdbstub::target::ext::base::reverse_exec::ReplayLogPosition;
use gdbstub::target::Target;

mod gdb;
mod target;

use log::{debug, info};
use target::{Event, RunEvent, SimTarget};

use clap::Parser;
use red_planet_core::board::{Board, Config};
use red_planet_core::simulator::SimulationAllocator;
use std::fs::File;
use stderrlog::LogLevelNum;

type Simulator = red_planet_core::simulator::Simulator<Board<SimulationAllocator>>;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Port to listen on for incoming gdb connections.
    #[arg(short, long)]
    gdb: Option<u16>,
    #[arg(short, long, default_value_t = true)]
    elf: bool,
    /// Binary file to execute.
    binary: String,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    stderrlog::new()
        .verbosity(LogLevelNum::Trace)
        .modules([module_path!(), "red_planet_core"])
        .init()
        .unwrap();

    let mut buf = Vec::new();

    use std::io::Read;
    let mut file = File::open(args.binary)?;
    file.read_to_end(&mut buf)?;

    let simulator = Simulator::new(|allocator| {
        let board = Board::new(allocator, Config::default());
        if args.elf {
            load_elf(&board, allocator, &buf).unwrap()
        } else {
            board.load_physical(allocator, 0x8000_0000, &buf);
        }
        board
    });

    if let Some(port) = args.gdb {
        run_gdb(simulator, port);
    } else {
        run(simulator);
    }

    Ok(())
}

fn load_elf(
    board: &Board<SimulationAllocator>,
    allocator: &mut SimulationAllocator,
    program_elf: &[u8],
) -> Result<(), goblin::error::Error> {
    // load ELF
    let elf_header = goblin::elf::Elf::parse(program_elf)?;

    // copy all in-memory sections from the ELF file into system RAM
    let sections = elf_header
        .section_headers
        .iter()
        .filter(|h| h.is_alloc() && h.sh_type != goblin::elf::section_header::SHT_NOBITS);

    for h in sections {
        debug!(
            "loading section {:?} into memory from [{:#010x?}..{:#010x?}]",
            elf_header.shdr_strtab.get_at(h.sh_name).unwrap(),
            h.sh_addr,
            h.sh_addr + h.sh_size,
        );

        let buf = &program_elf[h.file_range().unwrap()];
        board.load_physical(allocator, h.sh_addr as u32, buf);
    }

    Ok(())
}

fn run(mut simulator: Simulator) {
    loop {
        simulator.step();
        let (allocator, board) = simulator.inspect();
        if board.is_powered_down(allocator) {
            break;
        }
    }
}

fn run_gdb(simulator: Simulator, port: u16) {
    let mut target = SimTarget::new(simulator);

    let connection: TcpStream = wait_for_gdb_connection(port).unwrap();

    let debugger = GdbStub::new(connection);

    match debugger.run_blocking::<GdbBlockingEventLoop>(&mut target) {
        Ok(disconnect_reason) => match disconnect_reason {
            DisconnectReason::Disconnect => {
                println!("Client disconnected")
            }
            DisconnectReason::TargetExited(code) => {
                println!("Target exited with code {}", code)
            }
            DisconnectReason::TargetTerminated(sig) => {
                println!("Target terminated with signal {}", sig)
            }
            DisconnectReason::Kill => println!("GDB sent a kill command"),
        },
        Err(e) => {
            if e.is_target_error() {
                println!("target encountered a fatal error",)
            } else if e.is_connection_error() {
                let (e, kind) = e.into_connection_error().unwrap();
                println!("connection error: {:?} - {}", kind, e,)
            } else {
                println!("gdbstub encountered a fatal error")
            }
        }
    }
}

fn wait_for_gdb_connection(port: u16) -> io::Result<TcpStream> {
    let sockaddr = format!("localhost:{}", port);
    info!("Waiting for a GDB connection on {:?}...", sockaddr);
    let sock = TcpListener::bind(sockaddr)?;
    let (stream, addr) = sock.accept()?;

    info!("Debugger connected from {}", addr);
    Ok(stream)
}

enum GdbBlockingEventLoop {}

// The `run_blocking::BlockingEventLoop` groups together various callbacks
// the `GdbStub::run_blocking` event loop requires you to implement.
impl run_blocking::BlockingEventLoop for GdbBlockingEventLoop {
    type Target = SimTarget;
    type Connection = TcpStream;

    type StopReason = SingleThreadStopReason<u32>;

    // Invoked immediately after the target's `resume` method has been
    // called. The implementation should block until either the target
    // reports a stop reason, or if new data was sent over the connection.
    fn wait_for_stop_reason(
        target: &mut SimTarget,
        conn: &mut Self::Connection,
    ) -> Result<
        run_blocking::Event<SingleThreadStopReason<u32>>,
        run_blocking::WaitForStopReasonError<
            <Self::Target as Target>::Error,
            <Self::Connection as Connection>::Error,
        >,
    > {
        let poll_incoming_data = || {
            // gdbstub takes ownership of the underlying connection, so the `borrow_conn`
            // method is used to borrow the underlying connection back from the stub to
            // check for incoming data.
            conn.peek().map(|b| b.is_some()).unwrap_or(true)
        };

        match target.run(poll_incoming_data) {
            RunEvent::IncomingData => {
                let byte = conn
                    .read()
                    .map_err(run_blocking::WaitForStopReasonError::Connection)?;
                Ok(run_blocking::Event::IncomingData(byte))
            }
            RunEvent::Event(event) => {
                let stop_reason = match event {
                    Event::DoneStep => SingleThreadStopReason::DoneStep,
                    Event::ReachedStart => SingleThreadStopReason::ReplayLog {
                        tid: None,
                        pos: ReplayLogPosition::Begin,
                    },
                    Event::PoweredDown => SingleThreadStopReason::DoneStep, //Terminated(Signal::SIGSTOP),
                    Event::Break => SingleThreadStopReason::SwBreak(()),
                };

                Ok(run_blocking::Event::TargetStopped(stop_reason))
            }
        }
    }

    // Invoked when the GDB client sends a Ctrl-C interrupt.
    fn on_interrupt(
        _target: &mut SimTarget,
    ) -> Result<Option<SingleThreadStopReason<u32>>, <SimTarget as Target>::Error> {
        Ok(Some(SingleThreadStopReason::Signal(Signal::SIGINT)))
    }
}
