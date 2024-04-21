//! Provides a generic board built around the SiFive FE310-G002 SoC.

use crate::bus::{Bus, PureAccessResult};
use crate::core::{Config, ConnectedCore, Core};
use crate::resources::ram::Ram;
use crate::resources::rom::Rom;
use crate::simulator::Simulatable;
use crate::system_bus::{Slave, SystemBus};
use crate::{address_range, Allocator};
use std::ops::Deref;
use std::rc::Rc;

/// RISC-V hardware platform representing a board built around the SiFive FE310-G002 SoC.
///
/// This currently is a single-core board, with a single-hart core.
/// Multiprocessing and hardware multithreading are not supported.
///
/// > A RISC-V hardware platform can contain one or more RISC-V-compatible processing cores together
/// > with other non-RISC-V-compatible cores, fixed-function accelerators, various physical memory
/// > structures, I/O devices, and an interconnect structure to allow the components to communicate.
#[derive(Debug)]
pub struct Board<A: Allocator> {
    /// The single core of this board. Multiprocessing is not supported.
    core: ConnectedCore<A, Interconnect<A>>,
    /// Interconnect structure
    system_bus: Interconnect<A>,
    // Reset Vector ROM (4 KiB, mapped to 0x1000)
    reset_vector_rom: Rc<Rom<A>>,
    // Data Tightly Integrated Memory (16 KiB, mapped to 0x8000_0000)
    dtim: Rc<Ram<A>>,
}

impl<A: Allocator> Board<A> {
    pub fn new(allocator: &mut A) -> Self {
        let reset_vector_rom = Rc::new(
            Rom::new(
                allocator,
                0x1000,
                &[
                    0x97, 0x02, 0x00, 0x00, // auipc t0, 0
                    0x03, 0xa3, 0xc2, 0xff, // lw t1, -4(t0)
                    0x13, 0x13, 0x33, 0x00, // slli t1, t1, 0x3
                    0xb3, 0x82, 0x62, 0x00, // add t0, t0, t1
                    0x83, 0xa2, 0xc2, 0x0f, // lw t0, 252(t0)
                    0x67, 0x80, 0x02, 0x00, // jr t0
                ],
            )
            .unwrap(),
        );

        let dtim = Rc::new(Ram::new(allocator, 0x4000).unwrap());

        let system_bus = Rc::new(
            SystemBus::new()
                .with_resource(
                    Rc::clone(&reset_vector_rom) as Rc<dyn Slave<A>>,
                    [(address_range![0x1000, 0x1FFF], reset_vector_rom.range())],
                )
                .unwrap()
                .with_resource(
                    Rc::clone(&dtim) as Rc<dyn Slave<A>>,
                    [(address_range![0x8000_0000, 0x8000_3FFF], dtim.range())],
                )
                .unwrap(),
        );

        let core = Core::new(
            allocator,
            Config {
                // The FE310-G002 does not support misaligned accesses, but traps instead requiring
                // software emulation. We diverge from the spec and directly support misaligned
                // access in our emulated hardware.
                support_misaligned_memory_access: true,
                reset_vector: 0x1004,
            },
        );
        let core = core.connect(system_bus.clone());

        Self {
            core,
            reset_vector_rom,
            dtim,
            system_bus,
        }
    }

    pub fn core(&self) -> &ConnectedCore<A, impl Bus<A>> {
        &self.core
    }

    pub fn system_bus(&self) -> &SystemBus<A> {
        &self.system_bus
    }

    pub fn reset_vector_rom(&self) -> &Rom<A> {
        &self.reset_vector_rom
    }

    pub fn dtim(&self) -> &Ram<A> {
        &self.dtim
    }

    /// Force board back to its reset state.
    pub fn reset(&self, allocator: &mut A) {
        self.core.reset(allocator);
        self.dtim.reset(allocator);
    }
}

impl<A: Allocator> Simulatable<A> for Board<A> {
    fn tick(&self, allocator: &mut A) {
        self.core.tick(allocator)
    }

    fn drop(self, allocator: &mut A) {
        self.core.drop(allocator);
    }
}

// TODO: In the past this was a Rc<RefCell<SystemBus<A>>>, we might need that back in the future.
type Interconnect<A> = Rc<SystemBus<A>>;

impl<A: Allocator> Bus<A> for Interconnect<A> {
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        self.deref().read(buf, allocator, address)
    }

    fn read_pure(&self, buf: &mut [u8], allocator: &A, address: u32) -> PureAccessResult {
        self.deref().read_pure(buf, allocator, address)
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        self.deref().write(allocator, address, buf)
    }
}
