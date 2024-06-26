# RedPlanet
A RISC-V simulator with temporal state management.

## Prerequisites

Only the Rust compiler is needed to build and run this project, Check
[here](https://www.rust-lang.org/tools/install) for install instructions.

Some form of RISC-V capable `GDB` will also be useful for more complex debugging of the target
program. To check if your GDB installation is RISC-V capable run `set architecture riscv` in GDB. If
this outputs `The target architecture is set to "riscv".` you are all set.
Otherwise build or install the
[RISC-V GNU toolchain](https://github.com/riscv-collab/riscv-gnu-toolchain).

If you want to build some of the example programs you will also need the RISC-V GNU toolchain.

The RISCOF test suite to verify the simulation can be ran using
[docker](https://docs.docker.com/engine/install/).

## Building and running

To build the simulator run the following in the root directory of this project:

```bash
cargo build --release
```

This creates the executable `./target/release/red-planet-cli` with can be ran as a standalone
program.

You can also use the following to build and run in one command:
```bash
cargo run --release -- <ELF FILE>
```

## Building program's

You can find an example Makefile in `examples/demo` on how to build C program's for our simulator.
A linker script is also provided there.
Make sure you have the [RISC-V GNU toolchain](https://github.com/riscv-collab/riscv-gnu-toolchain)
installed and the right binary names are used at the top of the Makefile, as this can differ between
operating systems.

## Usage

Running `cargo run --release -- <ELF FILE>` will open a TUI aplication with your ELF file running.
At the top you can see the current status of the simulator. In the middle the output of the UART
device. And at the bottom a command prompt. To the left there is a pane to show the last log
messages.

You can use the arrow keys to move between the command pane and the UART pane. Anything you type in
the uart pane will be sent to the UART running in the simulation, but will not necessarily be echoed
back to you. It is a running program's job to do that.

The command pane at the bottom is used to control the simulation. The available commands are:

| short | long             | Result                             |
|-------|------------------|------------------------------------|
| c     | continue         | Run simulation forward             |
| rc    | reverse-continue | Run simulation backwards           |
| s     | step             | Do a single execution step         |
| rs    | reverse-step     | Undo a single step                 |
| df    | delete-future    | Delete all data from the current point onwards |
| g <N> | goto <STEP NUM>  | Goto a spcific step number         |
| p     | pauze            | Pause the simulation               |
|       | regs             | Read out all the regular registers |
| q     | quit             | Close the aplication               |

For more complex debugging tasks, GDB can be used. Start the simulator with the `--gdb 1234` flag
to make it open a port for GDB. You can now connect GDB at any point.

## RISCOF tests suite

The [RISCOF test suite](https://github.com/riscv-software-src/riscof) is a collection of programs to
test the compliance of a RISC-V implementation to the specs. It can be ran on our implantation
using the following steps:

### Setup

Before running the tests for the first time, the test suite must be cloned (this can take a while):

```bash
docker compose run --rm test make build-tests
```

### Running

To run the tests, use:

```bash
docker compose run --rm --build test
```

### Troubleshooting

If docker compose cannot find the `UID` or `GID` environment variables, make
sure they are exported from your shell (`export UID=$(id -u) GID=$(id -g)`).
If docker compose still complains, try adding them in a `.env` file.
