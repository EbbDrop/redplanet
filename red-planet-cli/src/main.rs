mod gdb;
mod target;
mod tcp;
mod tui;

use gdb::{run_server, GdbTarget};
use gdbstub::stub::DisconnectReason;
use goblin::elf::program_header::PT_LOAD;
use log::{debug, info, warn};
use target::{SharedTargetState, SimTarget};

use clap::Parser;
use red_planet_core::board::{Board, Config};
use red_planet_core::simulator::SimulationAllocator;
use std::fs::File;
use tcp::TcpStream;
use tokio::net::TcpListener;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::watch;
use tokio::{runtime, spawn};
use tui::{run_tui, TermSetupDropGard};

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
    let rt = runtime::Runtime::new()?;
    rt.block_on(start())?;
    rt.shutdown_background();
    Ok(())
}

async fn start() -> std::io::Result<()> {
    let args = Args::parse();

    tui_logger::init_logger(log::LevelFilter::Trace).unwrap();
    tui_logger::set_default_level(log::LevelFilter::Debug);
    // tui_logger::set_level_for_target(module_path!(), log::LevelFilter::Trace);
    // tui_logger::set_level_for_target("red_planet_core", log::LevelFilter::Trace);

    let mut buf = Vec::new();

    use std::io::Read;
    let mut file = File::open(args.binary)?;
    file.read_to_end(&mut buf)?;

    let mut simulator = Simulator::new(|allocator| {
        let board = Board::new(allocator, Config::default());
        if args.elf {
            load_elf(&board, allocator, &buf).unwrap()
        } else {
            board.load_physical(allocator, 0x8000_0000, &buf);
        }
        board
    });

    let terminal_drop_gard = TermSetupDropGard::new().unwrap();

    let (shared_state_sender, shared_state_receiver) = watch::channel(SharedTargetState::default());
    let (uart_sender, uart_receiver) = unbounded_channel();

    let (target, command_sender, event_receiver) =
        SimTarget::new(&mut simulator, shared_state_sender, uart_receiver);

    if let Some(port) = args.gdb {
        let gdb_target = GdbTarget::new(command_sender.clone(), event_receiver);
        spawn(run_gdb(gdb_target, port));
    } else {
        command_sender
            .send(target::command::Command::Continue)
            .unwrap();
    }

    spawn(run_tui(command_sender, shared_state_receiver, uart_sender));

    target.run(simulator).await;

    drop(terminal_drop_gard);

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
    let segments = elf_header
        .program_headers
        .iter()
        .filter(|h| h.p_type == PT_LOAD);

    for h in segments {
        debug!(
            "loading segment: file range [{:#010x?}..{:#010x?}] to pmem range [{:#010x?}..{:#010x?}] (virt {:#010x?})",
            h.p_offset,
            h.p_offset + h.p_filesz,
            h.p_paddr,
            h.p_paddr + h.p_memsz,
            h.p_vaddr,
        );

        let buf = &program_elf[h.file_range()];
        board.load_physical(allocator, h.p_paddr as u32, buf);
    }

    Ok(())
}

async fn run_gdb(mut target: GdbTarget, port: u16) {
    let connection = wait_for_gdb_connection(port).await.unwrap();

    match run_server(connection, &mut target).await {
        Ok(disconnect_reason) => match disconnect_reason {
            DisconnectReason::Disconnect => {
                warn!("Client disconnected")
            }
            DisconnectReason::TargetExited(code) => {
                warn!("Target exited with code {}", code)
            }
            DisconnectReason::TargetTerminated(sig) => {
                warn!("Target terminated with signal {}", sig)
            }
            DisconnectReason::Kill => warn!("GDB sent a kill command"),
        },
        Err(e) => match e {
            gdb::GdbError::Connection(e) => {
                warn!("connection error: {e}")
            }
            gdb::GdbError::Inner(e) => {
                warn!("{e}")
            }
            gdb::GdbError::TargetThreadStoped => {
                warn!("target encountered a fatal error")
            }
        },
    }
}

async fn wait_for_gdb_connection(port: u16) -> tokio::io::Result<TcpStream> {
    let sockaddr = format!("localhost:{}", port);
    info!("Waiting for a GDB connection on {:?}...", sockaddr);
    let sock = TcpListener::bind(sockaddr).await?;
    let (stream, addr) = sock.accept().await?;

    info!("Debugger connected from {}", addr);
    Ok(TcpStream(stream))
}
