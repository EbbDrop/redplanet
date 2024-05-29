use clap::Parser;
use red_planet_core::board::{Board, Config};
use red_planet_core::simulator::Simulator;
use std::fs::File;
use std::io::Read;
use stderrlog::LogLevelNum;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Binary file to execute.
    binary: String,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    stderrlog::new()
        .verbosity(LogLevelNum::Info)
        .init()
        .unwrap();

    let mut buf = Vec::new();

    let mut file = File::create(args.binary)?;
    file.read_to_end(&mut buf)?;

    run_binary(&buf);

    Ok(())
}

fn run_binary(binary: &[u8]) {
    let mut simulator = Simulator::new(|allocator| {
        let board = Board::new(allocator, Config::default());
        board.load_physical(allocator, 0x8000_0000, binary);
        board
    });
    loop {
        simulator.step();
        let (allocator, board) = simulator.inspect();
        if board.is_powered_down(allocator) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_binary;

    #[test]
    fn immediate_power_down() {
        run_binary(&[
            0xb7, 0x02, 0x10, 0x00, // lui t0, 0x100
            0x37, 0x53, 0x00, 0x00, // lui t1, 0x5
            0x13, 0x03, 0x53, 0x55, // addi t1, t1, 0x55
            0x23, 0xa0, 0x62, 0x00, // sw t1, 0(t0)
        ]);
    }
}
