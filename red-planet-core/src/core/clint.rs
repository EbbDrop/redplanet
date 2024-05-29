//! Core Local Interruptor

use space_time::allocator::Allocator;

use crate::bus::Bus;
use crate::interrupt::DynIrqCallback;

// https://github.com/qemu/qemu/blob/master/include/hw/intc/riscv_aclint.h#L74
pub const MTIMECMP_ADDR_HI: u32 = 0x0;
pub const MTIMECMP_ADDR_LO: u32 = MTIMECMP_ADDR_HI + 4;
pub const MTIME_ADDR_LO: u32 = 0x7ff8;
pub const MTIME_ADDR_HI: u32 = MTIME_ADDR_LO + 4;

#[derive(Debug)]
pub struct Clint<A: Allocator> {
    state: A::Id<State>,
    interrupt_callback: DynIrqCallback<A>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct State {
    mtime: u64,
    mtimecmp: u64,
}

impl State {
    fn new() -> Self {
        Self {
            mtime: 0,
            mtimecmp: 0,
        }
    }

    fn set_mtime_higher(&mut self, value: u32) {
        self.mtime = ((value as u64) << 32) & (self.mtime & 0xffffffff);
    }

    fn set_mtime_lower(&mut self, value: u32) {
        self.mtime = (self.mtime & 0xffffffff_00000000) & (value as u64 & 0xffffffff);
    }

    fn set_mtimecmp_higher(&mut self, value: u32) {
        self.mtimecmp = ((value as u64) << 32) & (self.mtimecmp & 0xffffffff);
    }

    fn set_mtimecmp_lower(&mut self, value: u32) {
        self.mtimecmp = (self.mtimecmp & 0xffffffff_00000000) & (value as u64 & 0xffffffff);
    }

    fn needs_interrupt(&self) -> bool {
        self.mtimecmp <= self.mtime
    }
}

impl<A: Allocator> Clint<A> {
    /// Create new Clint in reset state.
    pub fn new(allocator: &mut A, interrupt_callback: DynIrqCallback<A>) -> Self {
        Self {
            state: allocator.insert(State::new()),
            interrupt_callback,
        }
    }

    /// Restart the CLINT, setting everything to its reset state.
    ///
    /// mtime will be set to 0, mtimecmp will not be changed.
    pub fn reset(&self, allocator: &mut A) {
        allocator.get_mut(self.state).unwrap().mtime = 0;
    }

    pub fn step(&self, allocator: &mut A) {
        // TODO: Use some sort of external time to be independent of execution speed
        let mtime = &mut allocator.get_mut(self.state).unwrap().mtime;
        *mtime = mtime.wrapping_add(1);

        self.check_for_interrupt(allocator);
    }

    pub fn drop(self, allocator: &mut A) {
        allocator.remove(self.state).unwrap();
    }

    /// Read a u32 from the mmio registers.
    ///
    /// Only 4 byte alligned values will work
    pub fn read_u32(&self, allocator: &A, address: u32) -> u32 {
        match address {
            MTIMECMP_ADDR_HI => (allocator.get(self.state).unwrap().mtimecmp >> 32) as u32,
            MTIMECMP_ADDR_LO => allocator.get(self.state).unwrap().mtimecmp as u32,
            MTIME_ADDR_HI => (allocator.get(self.state).unwrap().mtime >> 32) as u32,
            MTIME_ADDR_LO => allocator.get(self.state).unwrap().mtime as u32,
            _ => 0,
        }
    }

    fn check_for_interrupt(&self, allocator: &mut A) {
        match allocator.get(self.state).unwrap().needs_interrupt() {
            true => self.interrupt_callback.raise(allocator),
            false => self.interrupt_callback.lower(allocator),
        }
    }

    /// Write an u32 to the mmio registers.
    ///
    /// Only 4 byte aligned values will work
    fn write_u32(&self, allocator: &mut A, address: u32, value: u32) {
        match address {
            MTIMECMP_ADDR_HI => allocator
                .get_mut(self.state)
                .unwrap()
                .set_mtimecmp_higher(value),
            MTIMECMP_ADDR_LO => allocator
                .get_mut(self.state)
                .unwrap()
                .set_mtimecmp_lower(value),
            MTIME_ADDR_HI => allocator
                .get_mut(self.state)
                .unwrap()
                .set_mtime_higher(value),
            MTIME_ADDR_LO => allocator
                .get_mut(self.state)
                .unwrap()
                .set_mtime_lower(value),
            _ => return,
        }
        self.check_for_interrupt(allocator);
    }

    /// Write an u64 to the mmio registers.
    ///
    /// Only 8 byte aligned values will work
    fn write_u64(&self, allocator: &mut A, address: u32, value: u64) {
        match address {
            MTIMECMP_ADDR_HI => allocator.get_mut(self.state).unwrap().mtimecmp = value,
            MTIME_ADDR_HI => allocator.get_mut(self.state).unwrap().mtime = value,
            _ => return,
        }
        self.check_for_interrupt(allocator);
    }

    pub fn read(&self, buf: &mut [u8], allocator: &A, address: u32) {
        if address != address & !0b11 {
            return;
        }
        match buf.len() {
            4 => {
                let v = self.read_u32(allocator, address);
                buf.copy_from_slice(&v.to_le_bytes())
            }
            8 => {
                let hi = self.read_u32(allocator, address) as u64;
                let lo = self.read_u32(allocator, address + 4) as u64;
                buf.copy_from_slice(&(hi << 32 | lo).to_le_bytes())
            }
            _ => {}
        }
    }

    pub fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        if address != address & !0b11 {
            return;
        }
        match buf {
            [a, b, c, d] => {
                self.write_u32(allocator, address, u32::from_le_bytes([*a, *b, *c, *d]));
            }
            [a, b, c, d, e, f, g, h] => {
                self.write_u64(
                    allocator,
                    address,
                    u64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h]),
                );
            }
            _ => {}
        }
    }
}

impl<A: Allocator> Bus<A> for Clint<A> {
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        self.read(buf, allocator, address)
    }

    fn read_debug(&self, buf: &mut [u8], allocator: &A, address: u32) {
        self.read(buf, allocator, address)
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        self.write(allocator, address, buf)
    }
}
