//! Provides a generic board built around the SiFive FE310-G002 SoC.

mod system_bus;

use crate::bus::Bus;
use crate::core::clint::{Clint, MTIMECMP_ADDR_LO, MTIME_ADDR_LO};
use crate::core::{Core, Interrupt};
use crate::resources::plic::Plic;
use crate::resources::ram::Ram;
use crate::resources::rom::Rom;
use crate::resources::uart::Uart;
use crate::simulator::Simulatable;
use crate::system_bus::AccessType;
use crate::{two_way_addr_map, Allocated, Allocator, Endianness};
use log::{debug, trace};
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
    core: Rc<Core<A, Interconnect<A>>>,
    system_bus: Rc<SystemBus<A>>,
}

impl<A: Allocator> Board<A> {
    pub fn new(allocator: &mut A, config: Config) -> Self {
        debug!("Creating board with config {config:?}");

        let memory_map = two_way_addr_map! {
            [0x0000_1000, 0x0000_FFFF] <=> Resource::Mrom,
            [0x0010_0000, 0x0010_0003] <=> Resource::PowerDown,
            [0x0200_0000, 0x0200_FFFF] <=> Resource::Clint,
            [0x0C00_0000, 0x0C20_0FFF] <=> Resource::Plic,
            [0x1000_0000, 0x1000_00FF] <=> Resource::Uart0,
            [0x2000_0000, 0x23FF_FFFF] <=> Resource::Flash,
            [0x8000_0000, 0xFFFF_FFFF] <=> Resource::Dram,
        };

        let mrom_range = memory_map.range_for(&Resource::Mrom).unwrap();
        let clint_range = memory_map.range_for(&Resource::Clint).unwrap();
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
                0x83, 0xa2, 0x02, 0x01, // lw     t0, 16(t0)
                0x67, 0x80, 0x02, 0x00, // jr     t0
                s[0], s[1], s[2], s[3], // .word start_address
            ]
        };

        let core = Rc::new_cyclic(|weak| {
            let mrom = Rom::new(allocator, mrom_range.size().unwrap(), &reset_vector).unwrap();

            let callback = Core::get_irq_callback(weak.clone(), Interrupt::MachineTimerInterrupt);
            let clint = Clint::new(allocator, callback);

            let callback =
                Core::get_irq_callback(weak.clone(), Interrupt::MachineExternalInterrupt);
            let plic = Plic::new(allocator, callback);

            let flash = Rom::new(allocator, flash_range.size().unwrap(), &config.flash).unwrap();

            let dram = Ram::new(allocator, dram_range.size().unwrap()).unwrap();

            let power_down = PowerDown::new(allocator);

            let system_bus = Rc::new_cyclic(|weak_bus| {
                let callback = SystemBus::get_plic_irq_callback(weak_bus.clone(), 3);
                let uart0 = Uart::new(allocator, callback);

                SystemBus {
                    memory_map,
                    mrom,
                    clint,
                    plic,
                    uart0,
                    flash,
                    dram,
                    power_down,
                }
            });

            Core::new(
                allocator,
                Rc::clone(&system_bus),
                crate::core::Config {
                    // At least one Hart must have ID 0 according to the spec.
                    hart_id: 0,
                    mtime_address: clint_range.start() + MTIME_ADDR_LO,
                    mtimecmp_address: clint_range.start() + MTIMECMP_ADDR_LO,
                    support_misaligned_memory_access: true,
                    strict_instruction_alignment: false,
                    reset_vector: mrom_range.start(),
                    // TODO: Research what address QEMU virt uses for this.
                    nmi_vector: mrom_range.start(),
                },
            )
        });

        let system_bus = core.system_bus().clone();

        Self { core, system_bus }
    }

    pub fn drop(self, allocator: &mut A) {
        // Unwrap safety: There should only be weak ptrs to the `core`.
        Rc::into_inner(self.core).unwrap().drop(allocator);

        // Unwrap safety: `core` is the only other owner and it has been dropped in the line above.
        Rc::into_inner(self.system_bus).unwrap().drop(allocator);
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

    /// Force board back to its reset state. Matches a hardware reset, meaning this is **not**
    /// equivalent to replacing this with [`Board::new`]. For example, some registers may not be
    /// cleared.
    pub fn reset(&self, allocator: &mut A) {
        self.core.reset(allocator);
        self.system_bus.dram.reset(allocator);
        self.system_bus.uart0.reset(allocator);
    }

    /// Power down the board. This makes ticks do nothing.
    pub fn power_down(&self, allocator: &mut A) {
        self.system_bus.power_down.power_down(allocator);
    }

    pub fn is_powered_down(&self, allocator: &A) -> bool {
        self.system_bus.power_down.is_powered_down(allocator)
    }

    /// Write a byte buffer into the physical address space.
    ///
    /// Bytes written to vacant, read-only, or I/O regions are ignored.
    pub fn load_physical(&self, allocator: &mut A, base_address: u32, buf: &[u8]) {
        if buf.is_empty() {
            return;
        }
        let memory_map = &self.system_bus.memory_map;
        let mut next_address = Some(base_address);
        while let Some(address) = next_address {
            if (address - base_address) as usize >= buf.len() {
                break;
            }

            let (range, resource) = memory_map.range_value(address);

            next_address = range.end().checked_add(1);

            let Some(resource) = resource else {
                continue;
            };

            match resource {
                Resource::Dram => {
                    const_assert!(usize::BITS >= 32);
                    let slice_start = (address - base_address) as usize;
                    let slice_end = ((range.end() - base_address) as usize).min(buf.len() - 1);
                    trace!(
                        "Writing buf[{slice_start:#0x}..={slice_end:#0x}] to DRAM at \
                         {address:#010x} through system bus"
                    );
                    let slice = &buf[slice_start..=slice_end];
                    self.system_bus.write(allocator, address, slice);
                }
                // Skip read-only
                Resource::Mrom => {}
                Resource::Flash => {}
                // Skip MMIO
                Resource::Uart0 => {}
                Resource::Clint => {}
                Resource::Plic => {}
                Resource::PowerDown => {}
            }
        }
    }

    /// Step the single core of this board once, if the board is not powered down.
    pub fn step(&self, allocator: &mut A) {
        if self.is_powered_down(allocator) {
            trace!("Not stepping board as it is powered down");
            return;
        }
        trace!("Stepping board");
        self.core.step(allocator);
        self.system_bus.clint.step(allocator);
    }
}

impl<A: Allocator> Simulatable<A> for Board<A> {
    fn tick(&self, allocator: &mut A) {
        self.step(allocator);
    }

    fn drop(self, allocator: &mut A) {
        self.drop(allocator);
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

#[derive(Debug)]
struct PowerDown<A: Allocator>(Allocated<A, bool>);

impl<A: Allocator> PowerDown<A> {
    fn new(allocator: &mut A) -> Self {
        Self(Allocated::new(allocator, false))
    }

    fn power_down(&self, allocator: &mut A) {
        *self.0.get_mut(allocator) = true;
    }

    fn is_powered_down(&self, allocator: &A) -> bool {
        *self.0.get(allocator)
    }
}

impl<A: Allocator> Bus<A> for PowerDown<A> {
    fn read(&self, _buf: &mut [u8], _allocator: &mut A, _address: u32) {
        // Reads are not supported, so do nothing.
    }

    fn read_debug(&self, _buf: &mut [u8], _allocator: &A, _address: u32) {
        // Reads are not supported, so do nothing.
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        // Ignore address, since it should be in the range 0x0..0x4, and the behavior is to round
        // down the address to the closest 4-byte aligned address.
        let _ = address;
        // If the lower 2 bytes are both 0x55, then we will power down.
        if let Some(&[0x55, 0x55]) = buf.get(..2) {
            debug!("Powering down board because 0x5555 was written");
            self.power_down(allocator);
        }
    }
}
