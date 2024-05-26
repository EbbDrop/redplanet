use space_time::allocator::Allocator;

use crate::system_bus::SystemBus;

use super::Core;

/// Collection of counter registers and associated read/write logic.
///
/// > RISC-V ISAs provide a set of up to 32×64-bit performance counters and timers that are
/// > accessible via unprivileged XLEN read-only CSR registers 0xC00–0xC1F (with the upper 32
/// > bits accessed via CSR registers 0xC80–0xC9F on RV32). The first three of these (CYCLE,
/// > TIME, and INSTRET) have dedicated functions (cycle count, real-time clock, and
/// > instructions-retired respectively), while the remaining counters, if implemented, provide
/// > programmable event counting.
///
/// > The RDCYCLE pseudoinstruction reads the low XLEN bits of the cycle CSR which holds a count
/// > of the number of clock cycles executed by the processor core on which the hart is running
/// > from an arbitrary start time in the past. RDCYCLEH is an RV32I instruction that reads bits
/// > 63–32 of the same cycle counter. The underlying 64-bit counter should never overflow in
/// > practice. The rate at which the cycle counter advances will depend on the implementation
/// > and operating environment. The execution environment should provide a means to determine
/// > the current rate (cycles/second) at which the cycle counter is incrementing.
///
/// > The RDTIME pseudoinstruction reads the low XLEN bits of the time CSR, which counts
/// > wall-clock real time that has passed from an arbitrary start time in the past. RDTIMEH is
/// > an RV32I-only instruction that reads bits 63–32 of the same real-time counter. The
/// > underlying 64-bit counter should never overflow in practice. The execution environment
/// > should provide a means of determining the period of the real-time counter (seconds/tick).
/// > The period must be constant. The real-time clocks of all harts in a single user
/// > application should be synchronized to within one tick of the real-time clock. The
/// > environment should provide a means to determine the accuracy of the clock.
///
/// > The RDINSTRET pseudoinstruction reads the low XLEN bits of the instret CSR, which counts
/// > the number of instructions retired by this hart from some arbitrary start point in the
/// > past. RDINSTRETH is an RV32I-only instruction that reads bits 63–32 of the same
/// > instruction counter. The underlying 64-bit counter should never overflow in practice.
///
/// > There is CSR space allocated for 29 additional unprivileged 64-bit hardware performance
/// > counters, hpmcounter3–hpmcounter31. For RV32, the upper 32 bits of these performance
/// > counters is accessible via additional CSRs hpmcounter3h–hpmcounter31h. These counters
/// > count platform-specific events and are configured via additional privileged registers. The
/// > number and width of these additional counters, and the set of events they count is
/// > platform-specific.
///
/// > The cycle, instret, and hpmcountern CSRs are read-only shadows of mcycle, minstret, and
/// > mhpmcountern, respectively. The time CSR is a read-only shadow of the memory-mapped mtime
/// > register. Analogously, on RV32I the cycleh, instreth and hpmcounternh CSRs are read-only
/// > shadows of mcycleh, minstreth and mhpmcounternh, respectively. On RV32I the timeh CSR is a
/// > read-only shadow of the upper 32 bits of the memory-mapped mtime register, while time shadows
/// > only the lower 32 bits of mtime.
#[derive(Debug, Clone)]
pub struct Counters {
    mcycle: u32,
    mcycleh: u32,
    minstret: u32,
    minstreth: u32,
    skip_next_mcycle_increment: bool,
    skip_next_minstret_increment: bool,
}

impl Default for Counters {
    fn default() -> Self {
        Self::new()
    }
}

impl Counters {
    pub fn new() -> Self {
        Self {
            // mcycle, mcycleh, minstret, and minstreth are reset to an arbitrary value
            mcycle: 0,
            mcycleh: 0,
            minstret: 0,
            minstreth: 0,
            skip_next_mcycle_increment: false,
            skip_next_minstret_increment: false,
        }
    }

    pub fn increment_cycle(&mut self) {
        if self.skip_next_mcycle_increment {
            self.skip_next_mcycle_increment = false;
            return;
        }
        self.mcycle = self.mcycle.wrapping_add(1);
        if self.mcycle == 0 {
            self.mcycleh = self.mcycleh.wrapping_add(1);
        }
    }

    pub fn increment_instret(&mut self) {
        if self.skip_next_minstret_increment {
            self.skip_next_minstret_increment = false;
            return;
        }
        self.minstret = self.minstret.wrapping_add(1);
        if self.minstret == 0 {
            self.minstreth = self.minstreth.wrapping_add(1);
        }
    }
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    pub fn read_cycle(&self, allocator: &mut A) -> u32 {
        self.read_mcycle(allocator)
    }

    pub fn read_cycleh(&self, allocator: &mut A) -> u32 {
        self.read_mcycleh(allocator)
    }

    pub fn read_instret(&self, allocator: &mut A) -> u32 {
        self.read_minstret(allocator)
    }

    pub fn read_instreth(&self, allocator: &mut A) -> u32 {
        self.read_minstreth(allocator)
    }

    pub fn read_hpmcounter(&self, allocator: &mut A, n: u8) -> u32 {
        self.read_mhpmcounter(allocator, n)
    }

    pub fn read_hpmcounterh(&self, allocator: &mut A, n: u8) -> u32 {
        self.read_mhpmcounterh(allocator, n)
    }

    pub fn read_mcycle(&self, allocator: &mut A) -> u32 {
        self.counters.get(allocator).mcycle
    }

    pub fn write_mcycle(&self, allocator: &mut A, value: u32, mask: u32) {
        let counters = self.counters.get_mut(allocator);
        counters.mcycle = counters.mcycle & !mask | value & mask;
        counters.skip_next_mcycle_increment = true;
    }

    pub fn read_mcycleh(&self, allocator: &mut A) -> u32 {
        self.counters.get(allocator).mcycleh
    }

    pub fn write_mcycleh(&self, allocator: &mut A, value: u32, mask: u32) {
        let counters = self.counters.get_mut(allocator);
        counters.mcycleh = counters.mcycleh & !mask | value & mask;
        counters.skip_next_mcycle_increment = true;
    }

    pub fn read_minstret(&self, allocator: &mut A) -> u32 {
        self.counters.get(allocator).minstret
    }

    pub fn write_minstret(&self, allocator: &mut A, value: u32, mask: u32) {
        let counters = self.counters.get_mut(allocator);
        counters.minstret = counters.minstret & !mask | value & mask;
        counters.skip_next_minstret_increment = true;
    }

    pub fn read_minstreth(&self, allocator: &mut A) -> u32 {
        self.counters.get(allocator).minstreth
    }

    pub fn write_minstreth(&self, allocator: &mut A, value: u32, mask: u32) {
        let counters = self.counters.get_mut(allocator);
        counters.minstreth = counters.minstreth & !mask | value & mask;
        counters.skip_next_minstret_increment = true;
    }

    pub fn read_mhpmcounter(&self, allocator: &mut A, n: u8) -> u32 {
        let _ = allocator;
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        0
    }

    pub fn write_mhpmcounter(&self, allocator: &mut A, n: u8, value: u32, mask: u32) {
        let _ = allocator;
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        // Writes are ignored
        let _ = value;
        let _ = mask;
    }

    pub fn read_mhpmcounterh(&self, allocator: &mut A, n: u8) -> u32 {
        let _ = allocator;
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        0
    }

    pub fn write_mhpmcounterh(&self, allocator: &mut A, n: u8, value: u32, mask: u32) {
        let _ = allocator;
        if !(3..=31).contains(&n) {
            panic!("invalid hpm counter number: {n}");
        }
        // Writes are ignored
        let _ = value;
        let _ = mask;
    }

    pub fn read_mhpmevent(&self, allocator: &mut A, n: u8) -> u32 {
        let _ = allocator;
        if !(3..=31).contains(&n) {
            panic!("invalid hpm event number: {n}");
        }
        0
    }

    pub fn write_mhpmevent(&self, allocator: &mut A, n: u8, value: u32, mask: u32) {
        let _ = allocator;
        if !(3..=31).contains(&n) {
            panic!("invalid hpm event number: {n}");
        }
        // Writes are ignored
        let _ = value;
        let _ = mask;
    }
}
