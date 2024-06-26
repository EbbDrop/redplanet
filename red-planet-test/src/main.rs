use clap::Parser;
use goblin::elf::program_header::PT_LOAD;
use red_planet_core::board::{Board, Config};
use red_planet_core::simulator::SimulationAllocator;
use std::fs::File;
use std::io::Read;
use std::io::Write;

type Simulator = red_planet_core::simulator::Simulator<Board<SimulationAllocator>>;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long, short)]
    // Signature file to output signature to
    signature: Option<String>,
    // Elf file to run
    elf: String,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let mut buf = Vec::new();

    let mut file = File::open(args.elf)?;
    file.read_to_end(&mut buf)?;

    let elf_header = goblin::elf::Elf::parse(&buf).expect("failed to parse elf file");

    let segments = elf_header
        .program_headers
        .iter()
        .filter(|h| h.p_type == PT_LOAD);

    let mut simulator = Simulator::new(|allocator| {
        let board = Board::new(allocator, Config::default());
        for h in segments {
            board.load_physical(allocator, h.p_paddr as u32, &buf[h.file_range()]);
        }
        board
    });

    // Run
    while {
        let (allocator, board) = simulator.inspect();
        !board.is_powered_down(allocator)
    } {
        simulator.step()
    }

    if let Some(path) = args.signature {
        let mut signature_start = None;
        let mut signature_end = None;
        for sym in elf_header.syms.iter() {
            let Some(name) = elf_header.strtab.get_at(sym.st_name) else {
                continue;
            };
            if name == "begin_signature" {
                signature_start = Some(sym.st_value as u32);
            } else if name == "end_signature" {
                signature_end = Some(sym.st_value as u32);
            }
        }
        let signature_start = signature_start.expect("missing symbol `begin_signature`");
        let signature_end = signature_end.expect("missing symbol `end_signature`");

        assert!(signature_start % 16 == 0);
        assert!(signature_end % 4 == 0);
        assert!(signature_start <= signature_end);

        let mut signature = Vec::new();

        let (allocator, board) = simulator.inspect();
        let mmu = board.core().mmu();
        for address in (signature_start..signature_end).step_by(4) {
            let word = mmu
                .read_word_debug(allocator, address)
                .expect("guest memory error while reading signature");
            signature.push(word);
        }

        let mut file = File::create(path)?;
        for word in signature {
            writeln!(file, "{word:08x}")?;
        }
    }

    Ok(())
}
