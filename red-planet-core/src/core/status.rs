use bitvec::{field::BitField, order::Lsb0, view::BitView};
use space_time::allocator::Allocator;

use crate::{system_bus::SystemBus, PrivilegeLevel, RawPrivilegeLevel};

use super::{Core, CsrReadResult, CsrWriteResult};

// Mask to be applied to mstatus to get sstatus.
const SSTATUS_MASK: u32 = 0b1111_1111_1000_1101_1110_0111_0111_0111;

/// Provides the mstatus, mstatush, and sstatus registers.
///
/// > The mstatus register is an MXLEN-bit read/write register [...]. The mstatus register keeps
/// > track of and controls the hart’s current operating state. A restricted view of mstatus appears
/// > as the sstatus register in the S-level ISA.
/// >
/// > For RV32 only, mstatush is a 32-bit read/write register [...].
#[derive(Debug, Clone)]
pub struct Status {
    mstatus: u32,
    mstatush: u32,
}

impl Default for Status {
    fn default() -> Self {
        Self::new()
    }
}

impl Status {
    pub fn new() -> Self {
        Self {
            mstatus: 0x0000_0000,
            mstatush: 0x0000_0000,
        }
    }

    /// Returns `true` if the MIE (M-mode Interrupt Enable) bit is set.
    pub fn mie(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::MIE]
    }

    /// Sets the MIE (M-mode Interrupt Enable) bit to `value`.
    pub fn set_mie(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::MIE, value);
    }

    /// Returns `true` if the SIE (S-mode Interrupt Enable) bit is set.
    pub fn sie(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::SIE]
    }

    /// Sets the SIE (S-mode Interrupt Enable) bit to `value`.
    pub fn set_sie(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::SIE, value);
    }

    /// Returns `true` if the MPIE (M-mode Previous Interrupt Enable) bit is set.
    pub fn mpie(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::MPIE]
    }

    /// Sets the MPIE (M-mode Previous Interrupt Enable) bit to `value`.
    pub fn set_mpie(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::MPIE, value);
    }

    /// Returns `true` if the SPIE (S-mode Previous Interrupt Enable) bit is set.
    pub fn spie(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::SPIE]
    }

    /// Sets the SPIE (S-mode Previous Interrupt Enable) bit to `value`.
    pub fn set_spie(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::SPIE, value);
    }

    /// Returns the privilege level encoded by the MPP (M-mode Previous Privilege level) field.
    ///
    /// The MPP field is **WARL**.
    pub fn mpp(&self) -> PrivilegeLevel {
        RawPrivilegeLevel::from_u2(
            self.mstatus.view_bits::<Lsb0>()[idx::MPP..(idx::MPP + 2)].load_le(),
        )
        .try_into()
        .unwrap()
    }

    /// Sets the privilege level encoded by the MPP (M-mode Previous Privilege level) field to
    /// `value`.
    ///
    /// The MPP field is **WARL**.
    pub fn set_mpp(&mut self, value: RawPrivilegeLevel) {
        let Ok(value) = PrivilegeLevel::try_from(value) else {
            // MPP is a WARL field, so ignore illegal values.
            return;
        };
        self.mstatus.view_bits_mut::<Lsb0>()[idx::MPP..(idx::MPP + 2)].store_le(value as u8);
    }

    /// Returns the privilege level encoded by the SPP (S-mode Previous Privilege level) field.
    ///
    /// The SPP field is **WARL**.
    pub fn spp(&self) -> PrivilegeLevel {
        RawPrivilegeLevel::from_u2(self.mstatus.view_bits::<Lsb0>()[idx::SPP] as u8)
            .try_into()
            .unwrap()
    }

    /// Sets the privilege level encoded by the SPP (S-mode Previous Privilege level) field to
    /// `value`.
    ///
    /// The SPP field is **WARL**.
    pub fn set_spp(&mut self, value: RawPrivilegeLevel) {
        match PrivilegeLevel::try_from(value) {
            Ok(value) if value <= PrivilegeLevel::Supervisor => {
                let bit = value as u8 != 0;
                self.mstatus.view_bits_mut::<Lsb0>().set(idx::SPP, bit);
            }
            _ => {} // SPP is a WARL field, so ignore illegal values.
        };
    }

    /// Returns `true` if the MPRV (Modify PRiVilege) bit is set.
    pub fn mprv(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::MPRV]
    }

    /// Sets the MPRV (Modify PRiVilege) bit to `value`.
    pub fn set_mprv(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::MPRV, value);
    }

    /// Returns `true` if the MXR (Make eXecutable Readable) bit is set.
    #[allow(dead_code)] // TODO: remove once method gets used.
    pub fn mxr(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::MXR]
    }

    /// Sets the MXR (Make eXecutable Readable) bit to `value`.
    pub fn set_mxr(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::MXR, value);
    }

    /// Returns `true` if the SUM (permit Supervisor User Memory access) bit is set.
    #[allow(dead_code)] // TODO: remove once method gets used.
    pub fn sum(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::SUM]
    }

    /// Sets the SUM (permit Supervisor User Memory access) bit to `value`.
    pub fn set_sum(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::SUM, value);
    }

    /// Returns `true` if the MBE (M-mode Big Endian) bit is set.
    pub fn mbe(&self) -> bool {
        self.mstatush.view_bits::<Lsb0>()[hidx::MBE]
    }

    /// Sets the MBE (M-mode Big Endian) bit to `value`.
    pub fn set_mbe(&mut self, value: bool) {
        self.mstatush.view_bits_mut::<Lsb0>().set(hidx::MBE, value);
    }

    /// Returns `true` if the SBE (S-mode Big Endian) bit is set.
    pub fn sbe(&self) -> bool {
        self.mstatush.view_bits::<Lsb0>()[hidx::SBE]
    }

    /// Sets the SBE (S-mode Big Endian) bit to `value`.
    pub fn set_sbe(&mut self, value: bool) {
        self.mstatush.view_bits_mut::<Lsb0>().set(hidx::SBE, value);
    }

    /// Returns `true` if the UBE (U-mode Big Endian) bit is set.
    pub fn ube(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::UBE]
    }

    /// Sets the UBE (U-mode Big Endian) bit to `value`.
    pub fn set_ube(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::UBE, value);
    }

    /// Returns `true` if the TVM (Trap Virtual Memory) bit is set.
    ///
    /// The TVM field is **WARL**.
    #[allow(dead_code)] // TODO: remove once method gets used.
    pub fn tvm(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::TVM]
    }

    /// Sets the TVM (Trap Virtual Memory) bit to `value`.
    ///
    /// The TVM field is **WARL**.
    pub fn set_tvm(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::TVM, value)
    }

    /// Returns `true` if the TW (Timeout Wait) bit is set.
    ///
    /// The TW field is **WARL**.
    #[allow(dead_code)] // TODO: remove once method gets used.
    pub fn tw(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::TW]
    }

    /// Sets the TW (Timeout Wait) bit to `value`.
    ///
    /// The TW field is **WARL**.
    pub fn set_tw(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::TW, value)
    }

    /// Returns `true` if the TSR (Trap SRET) bit is set.
    ///
    /// The TSR field is **WARL**.
    #[allow(dead_code)] // TODO: remove once method gets used.
    pub fn tsr(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::TSR]
    }

    /// Sets the TSR (Trap SRET) bit to `value`.
    ///
    /// The TSR field is **WARL**.
    pub fn set_tsr(&mut self, value: bool) {
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::TSR, value)
    }

    /// Returns the extension context status encoded by the FS (F extension Status) field.
    ///
    /// The FS field is **WARL**.
    pub fn fs(&self) -> ExtensionContextStatus {
        ExtensionContextStatus::from_u2(
            self.mstatus.view_bits::<Lsb0>()[idx::FS..(idx::FS + 2)].load_le(),
        )
    }

    /// Sets the extension context status encoded by the FS (F extension Status) field to `value`.
    ///
    /// The FS field is **WARL**.
    pub fn set_fs(&mut self, value: ExtensionContextStatus) {
        self.mstatus.view_bits_mut::<Lsb0>()[idx::FS..(idx::FS + 2)].store_le(value as u8);
        self.update_sd();
    }

    /// Returns the extension context status encoded by the VS (V extension Status) field.
    ///
    /// The VS field is **WARL**.
    pub fn vs(&self) -> ExtensionContextStatus {
        ExtensionContextStatus::from_u2(
            self.mstatus.view_bits::<Lsb0>()[idx::VS..(idx::VS + 2)].load_le(),
        )
    }

    /// Sets the extension context status encoded by the VS (V extension Status) field to `value`.
    ///
    /// The VS field is **WARL**.
    pub fn set_vs(&mut self, value: ExtensionContextStatus) {
        self.mstatus.view_bits_mut::<Lsb0>()[idx::VS..(idx::VS + 2)].store_le(value as u8);
        self.update_sd();
    }

    /// Returns the extension context status encoded by the XS (X extension Status) field.
    ///
    /// The XS field is **WARL**.
    pub fn xs(&self) -> ExtensionContextStatus {
        ExtensionContextStatus::from_u2(
            self.mstatus.view_bits::<Lsb0>()[idx::XS..(idx::XS + 2)].load_le(),
        )
    }

    /// Returns `true` if the SD (extension Status Dirty) bit is set.
    #[allow(dead_code)] // TODO: remove once method gets used.
    pub fn sd(&self) -> bool {
        self.mstatus.view_bits::<Lsb0>()[idx::SD]
    }

    fn update_sd(&mut self) {
        use ExtensionContextStatus::Dirty;
        let dirty = self.fs() == Dirty || self.vs() == Dirty || self.xs() == Dirty;
        self.mstatus.view_bits_mut::<Lsb0>().set(idx::SD, dirty);
    }
}

/// Bit indices into mstatus register.
mod idx {
    pub const SIE: usize = 1;
    pub const MIE: usize = 3;
    pub const SPIE: usize = 5;
    pub const UBE: usize = 6;
    pub const MPIE: usize = 7;
    pub const SPP: usize = 8;
    pub const VS: usize = 9;
    pub const MPP: usize = 11;
    pub const FS: usize = 13;
    pub const XS: usize = 15;
    pub const MPRV: usize = 17;
    pub const SUM: usize = 18;
    pub const MXR: usize = 19;
    pub const TVM: usize = 20;
    pub const TW: usize = 21;
    pub const TSR: usize = 22;
    pub const SD: usize = 31;
}

/// Bit indices into mstatush register.
mod hidx {
    pub const SBE: usize = 4;
    pub const MBE: usize = 5;
}

/// Possible values of the extension context status fields (FS, VS, XS) in the mstatus register.
///
/// > | Status | FS and VS Meaning | XS Meaning                   |
/// > | ------ | ----------------- | ---------------------------- |
/// > | 0      | Off               | All off                      |
/// > | 1      | Initial           | None dirty or clean, some on |
/// > | 2      | Clean             | None dirty, some clean       |
/// > | 3      | Dirty             | Some dirty                   |
///
/// > When an extension’s status is set to Off, any instruction that attempts to read or write the
/// > corresponding state will cause an illegal instruction exception. When the status is Initial,
/// > the corresponding state should have an initial constant value. When the status is Clean, the
/// > corresponding state is potentially different from the initial value, but matches the last
/// > value stored on a context swap. When the status is Dirty, the corresponding state has
/// > potentially been modified since the last context save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExtensionContextStatus {
    Off = 0,
    Initial = 1,
    Clean = 2,
    Dirty = 3,
}

impl ExtensionContextStatus {
    /// Convert a 2-bit value into an [`ExtensionContextStatus`].
    /// Panics if the value doesn't fit in 2 bits (`0..=3`).
    pub fn from_u2(value_u2: u8) -> Self {
        match value_u2 {
            0 => Self::Off,
            1 => Self::Initial,
            2 => Self::Clean,
            3 => Self::Dirty,
            _ => panic!("out of range u2 used"),
        }
    }
}

impl<A: Allocator, B: SystemBus<A>> Core<A, B> {
    pub fn read_mstatus(&self, allocator: &mut A) -> CsrReadResult {
        Ok(self.status.get(allocator).mstatus)
    }

    pub fn write_mstatus(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        let status = self.status.get_mut(allocator);

        let mask_bits = value.view_bits::<Lsb0>();
        let updated = status.mstatus & !mask | value & mask;
        let updated_bits = updated.view_bits::<Lsb0>();

        // Update the fields using the relevant setters to treat WARL fields correctly.
        if mask_bits[idx::SIE] {
            status.set_sie(updated_bits[idx::SIE]);
        }
        if mask_bits[idx::MIE] {
            status.set_mie(updated_bits[idx::MIE]);
        }
        if mask_bits[idx::SPIE] {
            status.set_spie(updated_bits[idx::SPIE]);
        }
        if mask_bits[idx::UBE] {
            status.set_ube(updated_bits[idx::UBE]);
        }
        if mask_bits[idx::MPIE] {
            status.set_mpie(updated_bits[idx::MPIE]);
        }
        if mask_bits[idx::SPP] {
            status.set_spp(RawPrivilegeLevel::from_u2(updated_bits[idx::SPP] as u8));
        }
        if mask_bits[idx::VS] | mask_bits[idx::VS + 1] {
            status.set_vs(ExtensionContextStatus::from_u2(
                updated_bits[idx::VS..(idx::VS + 2)].load_le(),
            ));
        }
        if mask_bits[idx::MPP] | mask_bits[idx::MPP + 1] {
            status.set_mpp(RawPrivilegeLevel::from_u2(
                updated_bits[idx::MPP..(idx::MPP + 2)].load_le(),
            ));
        }
        if mask_bits[idx::FS] | mask_bits[idx::FS + 1] {
            status.set_fs(ExtensionContextStatus::from_u2(
                updated_bits[idx::FS..(idx::FS + 2)].load_le(),
            ));
        }
        if mask_bits[idx::MPRV] {
            status.set_mprv(updated_bits[idx::MPRV]);
        }
        if mask_bits[idx::SUM] {
            status.set_sum(updated_bits[idx::SUM]);
        }
        if mask_bits[idx::MXR] {
            status.set_mxr(updated_bits[idx::MXR]);
        }
        if mask_bits[idx::TVM] {
            status.set_tvm(updated_bits[idx::TVM]);
        }
        if mask_bits[idx::TVM] {
            status.set_tvm(updated_bits[idx::TVM]);
        }
        if mask_bits[idx::TW] {
            status.set_tw(updated_bits[idx::TW]);
        }
        if mask_bits[idx::TSR] {
            status.set_tsr(updated_bits[idx::TSR]);
        }
        // Ignore read-only fields, and the remaining WPRI fields.
        Ok(())
    }

    pub fn read_mstatush(&self, allocator: &mut A) -> CsrReadResult {
        Ok(self.status.get(allocator).mstatush)
    }

    pub fn write_mstatush(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        let status = self.status.get_mut(allocator);
        let mask_bits = mask.view_bits::<Lsb0>();
        let value_bits = value.view_bits::<Lsb0>();
        // Update the fields using the relevant setters to treat WARL fields correctly.
        if mask_bits[hidx::MBE] {
            status.set_mbe(value_bits[hidx::MBE]);
        }
        if mask_bits[hidx::SBE] {
            status.set_sbe(value_bits[hidx::SBE]);
        }
        // Ignore the remaining WPRI fields.
        Ok(())
    }

    pub fn read_sstatus(&self, allocator: &mut A) -> CsrReadResult {
        Ok(self.status.get(allocator).mstatus & SSTATUS_MASK)
    }

    pub fn write_sstatus(&self, allocator: &mut A, value: u32, mask: u32) -> CsrWriteResult {
        self.write_mstatus(allocator, value, mask & SSTATUS_MASK)
    }
}
