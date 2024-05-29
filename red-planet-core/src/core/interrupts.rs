use std::rc::Weak;

use bitvec::{array::BitArray, field::BitField, order::Lsb0, view::BitView};
use log::trace;
use space_time::allocator::Allocator;

use super::{Core, CsrReadResult, CsrWriteResult, Interrupt};
use crate::{
    interrupt::{DynIrqCallback, IrqCallback},
    system_bus::SystemBus,
    PrivilegeLevel,
};

const SUPERVISOR_SOFTWARE_INTERRUPT: usize = Interrupt::SupervisorSoftwareInterrupt as usize;
const MACHINE_SOFTWARE_INTERRUPT: usize = Interrupt::MachineSoftwareInterrupt as usize;
const SUPERVISOR_TIMER_INTERRUPT: usize = Interrupt::SupervisorTimerInterrupt as usize;
const MACHINE_TIMER_INTERRUPT: usize = Interrupt::MachineTimerInterrupt as usize;
const SUPERVISOR_EXTERNAL_INTERRUPT: usize = Interrupt::SupervisorExternalInterrupt as usize;
const MACHINE_EXTERNAL_INTERRUPT: usize = Interrupt::MachineExternalInterrupt as usize;

#[allow(clippy::identity_op)]
const VALID_INTERRUPTS_MASK: u16 = 0
    | (1 << SUPERVISOR_SOFTWARE_INTERRUPT)
    | (1 << MACHINE_SOFTWARE_INTERRUPT)
    | (1 << SUPERVISOR_TIMER_INTERRUPT)
    | (1 << MACHINE_TIMER_INTERRUPT)
    | (1 << SUPERVISOR_EXTERNAL_INTERRUPT)
    | (1 << MACHINE_EXTERNAL_INTERRUPT);

// Delegetable interrupts according to QEMU's implementation.
#[allow(clippy::identity_op)]
const DELEGATABLE_INTERRUPTS_MASK: u16 = 0
    | (1 << SUPERVISOR_SOFTWARE_INTERRUPT)
    | (1 << SUPERVISOR_TIMER_INTERRUPT)
    | (1 << SUPERVISOR_EXTERNAL_INTERRUPT);

#[derive(Debug, Clone)]
pub struct Interrupts {
    /// Array of booleans, with for each bit index matching an interrupts's code a bool indicating
    /// whether handling that interrupt should be delegated to S-mode (if not triggered in M-mode).
    delegate: BitArray<[u16; 1], Lsb0>,

    /// Software-writable bit that is ORed with [`seip_external`] to become the SEIP field of the
    /// final [`mip`] register.
    seip_external: bool,
    /// External interrupt bit that is ORed with [`seip_internal`] to become the SEIP field of the
    /// final [`mip`] register.
    seip_internal: bool,

    /// The final mip register as visible from guest software. This means the SEIP field is
    /// recomputed each time [`seip_external`] or [`seip_internal`] changes.
    mip: BitArray<[u16; 1], Lsb0>,

    /// The mie register.
    mie: BitArray<[u16; 1], Lsb0>,
}

impl Default for Interrupts {
    fn default() -> Self {
        Self::new()
    }
}

impl Interrupts {
    pub fn new() -> Self {
        Self {
            // TODO: Are these defaults correct?
            delegate: BitArray::new([0x0000_0000]),
            seip_external: false,
            seip_internal: false,
            mip: BitArray::new([0x0000_0000]),
            mie: BitArray::new([0x0000_0000]),
        }
    }

    pub fn should_delegate(&self, interrupt: Interrupt) -> bool {
        self.delegate[interrupt as usize]
    }

    /// Indicate whether there is an M-level external interrupt pending (MEIP).
    ///
    /// Controlled by the PLIC.
    pub fn set_m_external(&mut self, value: bool) {
        trace!("Setting mip.MEIP to {value}");
        self.mip.set(MACHINE_EXTERNAL_INTERRUPT, value);
    }

    /// Indicate whether there is an S-level external interrupt pending (SEIP).
    ///
    /// Controlled by the PLIC. Note that calling this with `false` does not mean the SEIP field
    /// will be set to `0`, since it is ORed with the (hidden) software-writable SEIP bit.
    pub fn set_s_external(&mut self, value: bool) {
        trace!("Setting mip.SEIP to {value}");
        self.seip_external = value;
        self.mip.set(
            SUPERVISOR_EXTERNAL_INTERRUPT,
            self.seip_external | self.seip_internal,
        );
    }

    /// Indicate whether there is an M-level timer interrupt pending (MTIP).
    ///
    /// Controlled externally based on memory-mapped mtime and mtimecmp registers.
    pub fn set_m_timer(&mut self, value: bool) {
        trace!("Setting mip.MTIP to {value}");
        self.mip.set(MACHINE_TIMER_INTERRUPT, value);
    }

    // set_s_timer is missing, since STIP is only controllable by M-mode guest code.

    /// Indicate that an M-level software interrupt is pending (MSIP).
    ///
    /// Note that it is not possible to clear this bit. That is only possible from guest code.
    ///
    /// Controlled by accesses to memory-mapped control registers.
    pub fn set_m_soft(&mut self) {
        trace!("Setting mip.MSIP to true");
        self.mip.set(MACHINE_SOFTWARE_INTERRUPT, true);
    }

    /// Indicate that an S-level software interrupt is pending (SSIP).
    ///
    /// Note that it is not possible to clear this bit. That is only possible from guest code.
    ///
    /// May be set to 1 by the PLIC, but is also settable from guest code.
    pub fn set_s_soft(&mut self) {
        trace!("Setting mip.SSIP to true");
        self.mip.set(SUPERVISOR_SOFTWARE_INTERRUPT, true);
    }

    /// Returns `Some(interrupt)` if there is an interrupt ready to be taken in M-mode. If there are
    /// multiple interrupts ready, `interrupt` will be the one with the highest priority.
    ///
    /// An interrupt is considered ready to be taken in M-mode if it is pending in mip, enabled by
    /// mie, and not delegated by mideleg.
    pub fn highest_priority_m_ready(&self) -> Option<Interrupt> {
        let ready = self.mip & self.mie & !self.delegate;
        [
            Interrupt::MachineExternalInterrupt,
            Interrupt::MachineSoftwareInterrupt,
            Interrupt::MachineTimerInterrupt,
            Interrupt::SupervisorExternalInterrupt,
            Interrupt::SupervisorSoftwareInterrupt,
            Interrupt::SupervisorTimerInterrupt,
        ]
        .into_iter()
        .find(|&i| ready[i as usize])
    }

    /// Returns `Some(interrupt)` if there is an interrupt ready to be taken in S-mode. If there are
    /// multiple interrupts ready, `interrupt` will be the one with the highest priority.
    ///
    /// An interrupt is considered ready to be taken in S-mode if it is pending in sip, and enabled
    /// by sie.
    pub fn highest_priority_s_ready(&self) -> Option<Interrupt> {
        let ready = self.mip & self.mie & self.delegate;
        [
            Interrupt::SupervisorExternalInterrupt,
            Interrupt::SupervisorSoftwareInterrupt,
            Interrupt::SupervisorTimerInterrupt,
        ]
        .into_iter()
        .find(|&i| ready[i as usize])
    }
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    pub(super) fn highest_priority_ready_interrupt(&self, allocator: &A) -> Option<Interrupt> {
        let privilege_mode = self.privilege_mode(allocator);
        let status = self.status.get(allocator);

        if privilege_mode == PrivilegeLevel::Machine && !status.mie() {
            return None;
        }

        let interrupts = self.interrupts.get(allocator);

        if let interrupt @ Some(_) = interrupts.highest_priority_m_ready() {
            return interrupt;
        }

        if privilege_mode == PrivilegeLevel::Supervisor && !status.sie() {
            return None;
        }

        interrupts.highest_priority_s_ready()
    }
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    pub fn read_mideleg(&self, allocator: &mut A) -> CsrReadResult {
        Ok(self.interrupts.get(allocator).delegate.load_le())
    }

    /// The mideleg register is **WARL**.
    pub fn write_mideleg(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        let mideleg = &mut self.interrupts.get_mut(allocator).delegate;
        let mask = mask as u16 & DELEGATABLE_INTERRUPTS_MASK;
        mideleg.store_le(mideleg.load_le::<u16>() & !mask | value as u16 & mask);
        Ok(())
    }

    pub fn read_mip(&self, allocator: &mut A) -> CsrReadResult {
        Ok(self.interrupts.get(allocator).mip.load_le())
    }

    pub fn write_mip(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        let mask = mask.view_bits::<Lsb0>();
        let value = value.view_bits::<Lsb0>();

        // Writes to MEIP, MTIP, and MSIP are ignored. Their values are managed externally.
        // MEIP is managed by the PLIC.
        // MTIP is set/cleared based on the memory-mapped mtime and mtimecmp registers.
        // MSIP is written by accesses to memory-mapped control registers.

        let interrupts = &mut self.interrupts.get_mut(allocator);

        if mask[SUPERVISOR_EXTERNAL_INTERRUPT] {
            interrupts.seip_internal = value[SUPERVISOR_EXTERNAL_INTERRUPT];
            interrupts.mip.set(
                SUPERVISOR_EXTERNAL_INTERRUPT,
                interrupts.seip_external | interrupts.seip_internal,
            );
        }

        if mask[SUPERVISOR_TIMER_INTERRUPT] {
            interrupts.mip.set(
                SUPERVISOR_TIMER_INTERRUPT,
                value[SUPERVISOR_TIMER_INTERRUPT],
            );
        }

        if mask[SUPERVISOR_SOFTWARE_INTERRUPT] {
            interrupts.mip.set(
                SUPERVISOR_SOFTWARE_INTERRUPT,
                value[SUPERVISOR_SOFTWARE_INTERRUPT],
            );
        }

        Ok(())
    }

    pub fn read_mie(&self, allocator: &mut A) -> CsrReadResult {
        Ok(self.interrupts.get(allocator).mie.load_le())
    }

    pub fn write_mie(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        let mie = &mut self.interrupts.get_mut(allocator).mie;
        let mask = mask as u16 & VALID_INTERRUPTS_MASK;
        mie.store_le(mie.load_le::<u16>() & !mask | value as u16 & mask);
        Ok(())
    }

    pub fn read_sip(&self, allocator: &mut A) -> CsrReadResult {
        let interrupts = self.interrupts.get(allocator);
        Ok((interrupts.mip & interrupts.delegate).load_le())
    }

    pub fn write_sip(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        let mask = mask.view_bits::<Lsb0>();
        let value = value.view_bits::<Lsb0>();

        // SEIP and STIP are read-only in sip, so writes to it are ignored.

        let interrupts = &mut self.interrupts.get_mut(allocator);

        if mask[SUPERVISOR_SOFTWARE_INTERRUPT] {
            interrupts.mip.set(
                SUPERVISOR_SOFTWARE_INTERRUPT,
                value[SUPERVISOR_SOFTWARE_INTERRUPT],
            );
        }

        Ok(())
    }

    pub fn read_sie(&self, allocator: &mut A) -> CsrReadResult {
        let interrupts = self.interrupts.get(allocator);
        Ok((interrupts.mie & interrupts.delegate).load_le())
    }

    pub fn write_sie(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        let interrupts = self.interrupts.get_mut(allocator);
        let delegate = interrupts.delegate.load_le::<u16>();
        // Since we are masking with `delegate`, it is not needed to also mask with
        // VALID_INTERRUPTS_MASK (or DELEGETABLE_INTERRUPTS_MASK).
        let mask = mask as u16 & delegate;
        let mie = &mut interrupts.mie;
        mie.store_le(mie.load_le::<u16>() & !mask | value as u16 & mask);
        Ok(())
    }

    fn raise(&self, code: Interrupt, allocator: &mut A) {
        let interrupts = self.interrupts.get_mut(allocator);
        match code {
            Interrupt::SupervisorSoftwareInterrupt => interrupts.set_s_soft(),
            Interrupt::MachineSoftwareInterrupt => interrupts.set_m_soft(),
            Interrupt::SupervisorTimerInterrupt => {}
            Interrupt::MachineTimerInterrupt => interrupts.set_m_timer(true),
            Interrupt::SupervisorExternalInterrupt => interrupts.set_s_external(true),
            Interrupt::MachineExternalInterrupt => interrupts.set_m_external(true),
        }
    }

    fn lower(&self, code: Interrupt, allocator: &mut A) {
        let interrupts = self.interrupts.get_mut(allocator);
        match code {
            Interrupt::SupervisorSoftwareInterrupt => {}
            Interrupt::MachineSoftwareInterrupt => {}
            Interrupt::SupervisorTimerInterrupt => {}
            Interrupt::MachineTimerInterrupt => interrupts.set_m_timer(false),
            Interrupt::SupervisorExternalInterrupt => interrupts.set_s_external(false),
            Interrupt::MachineExternalInterrupt => interrupts.set_m_external(false),
        }
    }
}

struct CoreIrqCallback<A: Allocator, B: SystemBus<A>> {
    core: Weak<Core<A, B>>,
    code: Interrupt,
}

impl<A: Allocator, B: SystemBus<A> + 'static> Core<A, B> {
    pub fn get_irq_callback(core: Weak<Self>, code: Interrupt) -> DynIrqCallback<A> {
        DynIrqCallback(Box::new(CoreIrqCallback { core, code }))
    }
}

impl<A: Allocator, B: SystemBus<A>> IrqCallback<A> for CoreIrqCallback<A, B> {
    fn raise(&self, allocator: &mut A) {
        if let Some(core) = self.core.upgrade() {
            core.raise(self.code, allocator)
        }
    }

    fn lower(&self, allocator: &mut A) {
        if let Some(core) = self.core.upgrade() {
            core.lower(self.code, allocator)
        }
    }
}
