#[derive(Debug, Clone)]
pub struct Trap {
    mscratch: u32,
    mepc: u32,
    mcause: u32,
    mtval: u32,
    mtinst: u32,
    mtval2: u32,

    sscratch: u32,
    sepc: u32,
    scause: u32,
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
            mcause: 0,
            mtval: 0,
            mtinst: 0,
            mtval2: 0,

            sscratch: 0,
            sepc: 0,
            scause: 0,
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
        self.mepc = self.mepc & !mask | value & mask;
        self.mepc &= !0b1;
    }

    pub fn read_mcause(&self) -> u32 {
        self.mcause
    }

    pub fn write_mcause(&mut self, value: u32, mask: u32) {
        self.mcause = self.mcause & !mask | value & mask;
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

    pub fn read_scause(&self) -> u32 {
        self.scause
    }

    pub fn write_scause(&mut self, value: u32, mask: u32) {
        self.scause = self.scause & !mask | value & mask;
    }

    pub fn read_stval(&self) -> u32 {
        self.stval
    }

    pub fn write_stval(&mut self, value: u32, mask: u32) {
        self.stval = self.stval & !mask | value & mask;
    }
}
