use super::{Exception, Interrupt};

#[derive(Debug, Clone)]
pub struct Trap {
    mscratch: u32,
    mepc: u32,
    pub mcause: Cause,
    mtval: u32,
    mtinst: u32,
    mtval2: u32,

    sscratch: u32,
    sepc: u32,
    pub scause: Cause,
    stval: u32,
}

impl Default for Trap {
    fn default() -> Self {
        Self::new()
    }
}

impl Trap {
    pub fn new() -> Self {
        Self {
            mscratch: 0,
            mepc: 0,
            mcause: Cause::new(),
            mtval: 0,
            mtinst: 0,
            mtval2: 0,

            sscratch: 0,
            sepc: 0,
            scause: Cause::new(),
            stval: 0,
        }
    }

    pub fn read_mscratch(&self) -> u32 {
        self.mscratch
    }

    pub fn write_mscratch(&mut self, value: u32, mask: u32) {
        self.mscratch = self.mscratch & !mask | value & mask;
    }

    pub fn read_mepc(&self) -> u32 {
        self.mepc
    }

    pub fn write_mepc(&mut self, value: u32, mask: u32) {
        self.mepc = self.mepc & !mask | value & mask & !0b11;
    }

    pub fn read_mtval(&self) -> u32 {
        self.mtval
    }

    pub fn write_mtval(&mut self, value: u32, mask: u32) {
        self.mtval = self.mtval & !mask | value & mask;
    }

    pub fn read_mtinst(&self) -> u32 {
        self.mtinst
    }

    pub fn write_mtinst(&mut self, value: u32, mask: u32) {
        self.mtinst = self.mtinst & !mask | value & mask;
    }

    pub fn read_mtval2(&self) -> u32 {
        self.mtval2
    }

    pub fn write_mtval2(&mut self, value: u32, mask: u32) {
        self.mtval2 = self.mtval2 & !mask | value & mask;
    }

    pub fn read_sscratch(&self) -> u32 {
        self.sscratch
    }

    pub fn write_sscratch(&mut self, value: u32, mask: u32) {
        self.sscratch = self.sscratch & !mask | value & mask;
    }

    pub fn read_sepc(&self) -> u32 {
        self.sepc
    }

    pub fn write_sepc(&mut self, value: u32, mask: u32) {
        self.sepc = self.sepc & !mask | value & mask;
        self.sepc &= !0b1;
    }

    pub fn read_stval(&self) -> u32 {
        self.stval
    }

    pub fn write_stval(&mut self, value: u32, mask: u32) {
        self.stval = self.stval & !mask | value & mask;
    }
}

#[derive(Debug, Clone)]
pub struct Cause(u32);

impl Cause {
    pub fn new() -> Self {
        Self(0x0000_0000)
    }

    pub fn read(&self) -> u32 {
        self.0
    }

    pub fn write(&mut self, value: u32, mask: u32) {
        self.0 = self.0 & !mask | value & mask;
    }

    pub fn set(&mut self, cause: &TrapCause) {
        match cause {
            TrapCause::Exception(exception) => self.set_exception(Some(exception)),
            TrapCause::Interrupt(interrupt) => self.set_interrupt(Some(interrupt)),
        }
    }

    /// An `exception` of `None` indicates that the cause is unknown (results in all-zero code).
    pub fn set_exception(&mut self, exception: Option<&Exception>) {
        self.0 = exception.map(Exception::code).unwrap_or(0);
    }

    /// An `interrupt` of `None` indicates that the cause is unknown (results in all-zero code).
    pub fn set_interrupt(&mut self, interrupt: Option<&Interrupt>) {
        self.0 = 0x8000_0000 | interrupt.map(Interrupt::code).unwrap_or(0);
    }
}

#[derive(Debug, Clone)]
pub enum TrapCause {
    Exception(Exception),
    Interrupt(Interrupt),
}

impl From<Exception> for TrapCause {
    fn from(value: Exception) -> Self {
        Self::Exception(value)
    }
}

impl From<Interrupt> for TrapCause {
    fn from(value: Interrupt) -> Self {
        Self::Interrupt(value)
    }
}
