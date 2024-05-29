//! Platform-level interrupt controller

use bitvec::array::BitArray;
use bitvec::order::Msb0;
use bitvec::BitArr;
use space_time::allocator::Allocator;

use crate::bus::Bus;

use crate::interrupt::DynIrqCallback;

pub const PRIORITY_BASE_ADDR: u32 = 0x4;
pub const PRIORITY_LAST_ADDR: u32 = 0xD0;

pub const PENDING_BASE_ADDR: u32 = 0x1000;
pub const PENDING_LAST_ADDR: u32 = 0x1004;

pub const ENABLES_BASE_ADDR: u32 = 0x2000;
pub const ENABLES_LAST_ADDR: u32 = 0x2004;

pub const THRESHOLD_ADDR: u32 = 0x20_0000;
pub const CLAIMCOMPLETE_ADDR: u32 = 0x20_0004;

#[derive(Debug)]
pub struct Plic<A: Allocator> {
    state: A::Id<State>,
    interrupt_callback: DynIrqCallback<A>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct State {
    prioritys: [u32; 53],
    pending: BitArr!(for 53, in u32, Msb0),
    enabled: BitArr!(for 53, in u32, Msb0),
    priority_threshold: u32,
}

impl State {
    fn new() -> Self {
        Self {
            prioritys: [0; 53],
            pending: BitArray::ZERO,
            enabled: BitArray::ZERO,
            priority_threshold: 0,
        }
    }

    fn set_pending(&mut self, index: u8) {
        *self.pending.get_mut(index as usize).unwrap() = true;
    }

    fn set_complete(&mut self, index: u8) {
        *self.pending.get_mut(index as usize).unwrap() = false;
    }

    fn set_priority(&mut self, index: usize, value: u32) {
        self.prioritys[index] = value.min(7);
    }

    fn set_priority_threshold(&mut self, value: u32) {
        self.priority_threshold = value.min(7);
    }

    /// Returns 0 if no interrupts are pending
    fn highest_priority_pending(&self) -> u32 {
        let Some((idx, priority)) = self
            .prioritys
            .iter()
            .enumerate()
            .zip(self.pending)
            .zip(self.enabled)
            .filter(|((_, pending), enabled)| *enabled && *pending)
            .map(|(((idx, priority), _), _)| (idx as u32, *priority))
            .rev()
            .max_by(|(_, priority_a), (_, priority_b)| priority_a.cmp(priority_b))
        else {
            return 0;
        };

        if priority <= self.priority_threshold {
            return 0;
        }
        idx
    }

    fn claim_highest_priority_pending(&mut self) -> u32 {
        let idx = self.highest_priority_pending();
        if idx != 0 {
            *self.pending.get_mut(idx as usize).unwrap() = false;
        }
        idx
    }

    fn needs_interrupt(&self) -> bool {
        self.highest_priority_pending() != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddrAccessor {
    Priorities(usize),
    Pending(usize),
    Enabled(usize),
    Threshold,
    ClaimComplete,
}

impl AddrAccessor {
    fn from_address(address: u32) -> Option<Self> {
        match address {
            PRIORITY_BASE_ADDR..=PRIORITY_LAST_ADDR => {
                // `+ 1` to skip the interrupt 0 "no interrupt"
                Some(Self::Priorities(
                    (address - PRIORITY_BASE_ADDR + 1) as usize,
                ))
            }
            PENDING_BASE_ADDR..=PENDING_LAST_ADDR => {
                Some(Self::Pending((address - PENDING_BASE_ADDR) as usize))
            }
            ENABLES_BASE_ADDR..=ENABLES_LAST_ADDR => {
                Some(Self::Enabled((address - ENABLES_BASE_ADDR) as usize))
            }
            THRESHOLD_ADDR => Some(Self::Threshold),
            CLAIMCOMPLETE_ADDR => Some(Self::ClaimComplete),
            _ => None,
        }
    }
}

impl<A: Allocator> Plic<A> {
    /// Create new Plic in reset state.
    pub fn new(allocator: &mut A, interrupt_callback: DynIrqCallback<A>) -> Self {
        Self {
            state: allocator.insert(State::new()),
            interrupt_callback,
        }
    }

    pub fn reset(&self, allocator: &mut A) {
        *allocator.get_mut(self.state).unwrap() = State::new();
    }

    pub fn drop(self, allocator: &mut A) {
        allocator.remove(self.state).unwrap();
    }

    pub fn raise(&self, allocator: &mut A, index: u8) {
        allocator.get_mut(self.state).unwrap().set_pending(index)
    }

    pub fn lower(&self, _allocator: &mut A, _index: u8) {
        // The PLIC ignores lowers explicitly
    }

    fn check_for_interrupt(&self, allocator: &mut A) {
        match allocator.get(self.state).unwrap().needs_interrupt() {
            true => self.interrupt_callback.raise(allocator),
            false => self.interrupt_callback.lower(allocator),
        }
    }

    fn read_u32(&self, allocator: &mut A, address: u32) -> u32 {
        let Some(address) = AddrAccessor::from_address(address) else {
            return 0;
        };
        match address {
            AddrAccessor::Priorities(i) => allocator.get(self.state).unwrap().prioritys[i],
            AddrAccessor::Enabled(i) => {
                allocator.get(self.state).unwrap().enabled.as_raw_slice()[i]
            }
            AddrAccessor::Pending(i) => {
                allocator.get(self.state).unwrap().pending.as_raw_slice()[i]
            }
            AddrAccessor::Threshold => allocator.get(self.state).unwrap().priority_threshold,
            AddrAccessor::ClaimComplete => {
                let claim = allocator
                    .get_mut(self.state)
                    .unwrap()
                    .claim_highest_priority_pending();
                self.check_for_interrupt(allocator);
                claim
            }
        }
    }

    fn read_u32_debug(&self, allocator: &A, address: u32) -> u32 {
        let Some(address) = AddrAccessor::from_address(address) else {
            return 0;
        };
        let state = allocator.get(self.state).unwrap();
        match address {
            AddrAccessor::Priorities(i) => state.prioritys[i],
            AddrAccessor::Enabled(i) => state.enabled.as_raw_slice()[i],
            AddrAccessor::Pending(i) => state.pending.as_raw_slice()[i],
            AddrAccessor::Threshold => state.priority_threshold,
            AddrAccessor::ClaimComplete => state.highest_priority_pending(),
        }
    }

    fn write_u32(&self, allocator: &mut A, address: u32, value: u32) {
        let Some(address) = AddrAccessor::from_address(address) else {
            return;
        };
        let state = allocator.get_mut(self.state).unwrap();
        match address {
            AddrAccessor::Priorities(i) => state.set_priority(i, value),
            AddrAccessor::Enabled(i) => {
                let value = if i == 0 { value & 0x8000_0000 } else { value };
                state.enabled.as_raw_mut_slice()[i] = value;
            }
            AddrAccessor::Pending(i) => {
                let value = if i == 0 { value & 0x8000_0000 } else { value };
                state.pending.as_raw_mut_slice()[i] = value;
            }
            AddrAccessor::Threshold => state.set_priority_threshold(value),
            AddrAccessor::ClaimComplete => {
                if (1..=52).contains(&value) {
                    state.set_complete(value as u8)
                }
            }
        }
        self.check_for_interrupt(allocator);
    }
}

impl<A: Allocator> Bus<A> for Plic<A> {
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        if address != address & !0b11 {
            return;
        }
        if buf.len() == 4 {
            let v = self.read_u32(allocator, address);
            buf.copy_from_slice(&v.to_le_bytes())
        }
    }

    fn read_debug(&self, buf: &mut [u8], allocator: &A, address: u32) {
        if address != address & !0b11 {
            return;
        }
        if buf.len() == 4 {
            let v = self.read_u32_debug(allocator, address);
            buf.copy_from_slice(&v.to_le_bytes())
        }
    }

    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        if address != address & !0b11 {
            return;
        }
        if let [a, b, c, d] = buf {
            self.write_u32(allocator, address, u32::from_le_bytes([*a, *b, *c, *d]));
        }
    }
}
