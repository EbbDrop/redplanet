//! Provides a generic board built around the SiFive FE310-G002 SoC.

mod system_bus;

use crate::bus::Bus;
use crate::core::Core;
use crate::resources::ram::Ram;
use crate::resources::rom::Rom;
use crate::resources::uart::Uart;
use crate::simulator::Simulatable;
use crate::system_bus::AccessType;
use crate::{two_way_addr_map, Allocator, Endianness};
use std::ops::Deref;
use std::rc::Rc;
use system_bus::{Resource, SystemBus};

#[derive(Debug, Clone)]
pub struct Config {
    /// If `true`, the reset vector in MROM will jump to flash, otherwise to the start of RAM.
    pub boot_to_flash: bool,
    /// M-mode endianness
    pub endianness: Endianness,
    /// Contents of flash (max 64 MiB)
    pub flash: Vec<u8>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            boot_to_flash: false,
            endianness: Endianness::LE,
            flash: Vec::default(),
        }
    }
}

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
    core: Core<A, Interconnect<A>>,
    system_bus: Rc<SystemBus<A>>,
}

impl<A: Allocator> Board<A> {
    pub fn new(allocator: &mut A, config: Config) -> Self {
        let memory_map = two_way_addr_map! {
            [0x0000_1000, 0x0000_FFFF] <=> Resource::Mrom,
            [0x1000_0000, 0x1000_00FF] <=> Resource::Uart0,
            [0x2000_0000, 0x23FF_FFFF] <=> Resource::Flash,
            [0x8000_0000, 0xFFFF_FFFF] <=> Resource::Dram,
        };

        let mrom_range = memory_map.range_for(&Resource::Mrom).unwrap();
        let flash_range = memory_map.range_for(&Resource::Flash).unwrap();
        let dram_range = memory_map.range_for(&Resource::Dram).unwrap();

        let start_address: u32 = if config.boot_to_flash {
            flash_range.start()
        } else {
            dram_range.start()
        };

        let reset_vector = {
            let s: [u8; 4] = match config.endianness {
                Endianness::LE => start_address.to_le_bytes(),
                Endianness::BE => start_address.to_be_bytes(),
            };
            [
                0x97, 0x02, 0x00, 0x00, // auipc  t0, 0x0
                0x73, 0x25, 0x40, 0xf1, // csrr   a0, mhartid
                0x83, 0xa2, 0x82, 0x01, // lw     t0, 16(t0)
                0x67, 0x80, 0x02, 0x00, // jr     t0
                s[0], s[1], s[2], s[3], // .word start_address
            ]
        };

        let mrom = Rom::new(allocator, mrom_range.size().unwrap(), &reset_vector).unwrap();

        let flash = Rom::new(allocator, flash_range.size().unwrap(), &config.flash).unwrap();

        let dram = Ram::new(allocator, dram_range.size().unwrap()).unwrap();

        let uart0 = Uart::new(allocator);

        let system_bus = Rc::new(SystemBus {
            memory_map,
            mrom,
            uart0,
            flash,
            dram,
        });

        let core = Core::new(
            allocator,
            Rc::clone(&system_bus),
            crate::core::Config {
                // At least one Hart must have ID 0 according to the spec.
                hart_id: 0,
                support_misaligned_memory_access: true,
                reset_vector: mrom_range.start(),
            },
        );

        Self { core, system_bus }
    }

    pub fn core(&self) -> &Core<A, impl crate::system_bus::SystemBus<A>> {
        &self.core
    }

    pub fn mrom(&self) -> &Rom<A> {
        &self.system_bus.mrom
    }

    pub fn flash(&self) -> &Rom<A> {
        &self.system_bus.flash
    }

    pub fn dram(&self) -> &Ram<A> {
        &self.system_bus.dram
    }

    pub fn uart0(&self) -> &Uart<A> {
        &self.system_bus.uart0
    }

    /// Force board back to its reset state.
    pub fn reset(&self, allocator: &mut A) {
        self.core.reset(allocator);
        self.system_bus.dram.reset(allocator);
        self.system_bus.uart0.reset(allocator);
    }

    /// Write a byte buffer into the physical address space.
    ///
    /// Bytes written to vacant, read-only, or I/O regions are ignored.
    pub fn load_physical(&self, allocator: &mut A, base_address: u32, buf: &[u8]) {
        let memory_map = &self.system_bus.memory_map;
        let mut next_address = Some(base_address);
        while let Some(address) = next_address {
            let (range, resource) = memory_map.range_value(address);

            next_address = range.end().checked_add(1);

            let Some(resource) = resource else {
                continue;
            };

            match resource {
                Resource::Dram => {
                    const_assert!(usize::BITS >= 32);
                    let slice_start = (address - base_address) as usize;
                    let slice_end = (range.end() - base_address) as usize;
                    let slice = &buf[slice_start..=slice_end];
                    self.system_bus.write(allocator, address, slice);
                }
                // Skip read-only
                Resource::Mrom => {}
                Resource::Flash => {}
                // Skip MMIO
                Resource::Uart0 => {}
            }
        }
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

type Interconnect<A> = Rc<SystemBus<A>>;

impl<A: Allocator> Bus<A> for Interconnect<A> {
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        self.deref().read(buf, allocator, address)
    }

    fn read_debug(&self, buf: &mut [u8], allocator: &A, address: u32) {
        self.deref().read_debug(buf, allocator, address)
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        self.deref().write(allocator, address, buf)
    }
}

impl<A: Allocator> crate::system_bus::SystemBus<A> for Interconnect<A> {
    fn accepts(&self, address: u32, size: usize, access_type: AccessType) -> bool {
        self.deref().accepts(address, size, access_type)
    }
}
