//! Implementation of an UART16550A as a simulatable device.

use crate::bus::Bus;
use crate::interrupt::DynIrqCallback;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use space_time::allocator::Allocator;
use thiserror::Error;

/// UART device implementation, unfinished and not conforming to any spec.
///
/// Resources:
/// - <https://uart16550.readthedocs.io>
/// - <https://github.com/qemu/qemu/blob/master/hw/char/serial.c>
#[derive(Debug)]
pub struct Uart<A: Allocator> {
    state: A::Id<State>,
    interrupt_callback: DynIrqCallback<A>,
}

/// State of an [`Uart`].
#[derive(Debug, Clone, Eq, PartialEq)]
struct State {
    /// Interrupt Enable Register
    ier: u8,
    /// Interrupt Identification Register
    iir: u8,
    /// Line Control Register
    lcr: u8,
    /// Line Status Register
    lsr: u8,
    /// Modem Status Register
    msr: u8,
    /// Divisor Latch Register
    dlr: u16,

    /// Receiver FIFO Interrupt Trigger Level (set by the FIFO Control Register).
    ///
    /// Expressed in bytes. The possible values are 1, 4, 8, or 14 bytes.
    rx_fifo_itl: u8,

    /// Receiver FIFO
    rx_fifo_buf: [u8; 16],
    rx_fifo_len: u8,

    /// Transmitter FIFO
    tx_fifo_buf: [u8; 16],
    tx_fifo_len: u8,
}

impl State {
    /// Returns the reset state.
    fn new() -> Self {
        Self {
            // Registers
            ier: 0x00,
            iir: 0xC1,
            lcr: 0x03,
            lsr: 0x60,
            msr: 0x00,
            dlr: 0x0000,
            // RX FIFO Interrupt Trigger Level is 14 bytes on reset
            rx_fifo_itl: 14,
            // Receiver FIFO
            rx_fifo_buf: [0; 16],
            rx_fifo_len: 0,
            // Transmitter FIFO
            tx_fifo_buf: [0; 16],
            tx_fifo_len: 0,
        }
    }

    /// Return `true` if enable receiver line status interrupt
    fn ier_rlsi(&self) -> bool {
        let bits = self.lsr.view_bits::<Lsb0>();
        bits[0] || bits[2]
    }

    /// Return `true` if enable receiver data interrupt is set
    fn ier_rdi(&self) -> bool {
        self.lsr.view_bits::<Lsb0>()[0]
    }

    /// Return `true` if enable Modem status interrupt is set
    fn ier_msi(&self) -> bool {
        self.lsr.view_bits::<Lsb0>()[3]
    }

    /// Returns `true` if the Divisor Latch Access Bit is `1`.
    fn dlab(&self) -> bool {
        (self.lcr >> 7) == 1
    }

    /// Returns `true` if the Data Ready indicator of the Line Status Register is `1`.
    fn lsr_dr(&self) -> bool {
        self.lsr.view_bits::<Lsb0>()[0]
    }

    /// Set the Data Ready indicator of the Line Status Register.
    fn set_lsr_dr(&mut self, value: bool) {
        self.lsr.view_bits_mut::<Lsb0>().set(0, value);
    }

    /// Returns `true` if the Overrun Error indicator of the Line Status Register is `1`.
    fn lsr_oe(&self) -> bool {
        self.lsr.view_bits::<Lsb0>()[1]
    }

    /// Set the Overrun Error indicator of the Line Status Register.
    fn set_lsr_oe(&mut self, value: bool) {
        self.lsr.view_bits_mut::<Lsb0>().set(1, value);
    }

    /// Returns `true` if the Transmitter Holding Register Empty indicator of the Line Status
    /// Register is `1`.
    #[allow(unused)]
    fn lsr_thre(&self) -> bool {
        self.lsr.view_bits::<Lsb0>()[5]
    }

    /// Set the Transmitter Holding Register Empty indicator of the Line Status Register.
    fn set_lsr_thre(&mut self, value: bool) {
        self.lsr.view_bits_mut::<Lsb0>().set(5, value);
    }

    /// Returns `true` if the Transmitter FIFO Empty indicator of the Line Status Register is `1`.
    #[allow(unused)]
    fn lsr_tfe(&self) -> bool {
        self.lsr.view_bits::<Lsb0>()[6]
    }

    /// Set the Transmitter FIFO Empty indicator of the Line Status Register.
    fn set_lsr_tfe(&mut self, value: bool) {
        self.lsr.view_bits_mut::<Lsb0>().set(6, value);
    }

    /// Return true if any of the interrupt triggering bits are set
    fn lsr_int_any(&self) -> bool {
        self.lsr & 0x1E != 0
    }

    /// Return true if any of the delta bits are set
    fn msr_any_delta(&self) -> bool {
        self.msr & 0x0F != 0
    }

    /// Returns `true` if the UART is operational, which is the case if the divisor latch value is
    /// non-zero.
    fn is_operational(&self) -> bool {
        self.dlr != 0
    }

    /// Returns the bitmask to be applied to each character.
    fn char_mask(&self) -> u8 {
        (((1 << ((self.lcr & 0b11) + 1)) - 1) << 4) | 0xF
    }
}

#[derive(Error, Debug)]
pub enum ReadError {
    #[error("cannot read from write-only register")]
    WriteOnly(&'static str),
    #[error("no register mapped to address {0:#x}")]
    AddressInvalid(u8),
}

#[derive(Error, Debug)]
pub enum WriteError {
    #[error("cannot write to read-only register")]
    ReadOnly(&'static str),
    #[error("no register mapped to address {0:#x}")]
    AddressInvalid(u8),
}

impl<A: Allocator> Uart<A> {
    /// Create new UART in reset state.
    pub fn new(allocator: &mut A, interrupt_callback: DynIrqCallback<A>) -> Self {
        Self {
            state: allocator.insert(State::new()),
            interrupt_callback,
        }
    }

    /// Restart the UART, setting everything to its reset state.
    pub fn reset(&self, allocator: &mut A) {
        *allocator.get_mut(self.state).unwrap() = State::new();
    }

    fn update_interrupt(&self, allocator: &mut A) {
        const UART_IIR_NO_INT: u8 = 0x01;
        const UART_IIR_RLSI: u8 = 0x06;
        const UART_IIR_RDI: u8 = 0x04;
        const UART_IIR_MSI: u8 = 0x00;

        let state = allocator.get_mut(self.state).unwrap();

        let new_irr = if state.ier_rlsi() && state.lsr_int_any() {
            UART_IIR_RLSI
        } else if state.ier_rdi() && state.lsr_dr() && (state.rx_fifo_len >= state.rx_fifo_itl) {
            UART_IIR_RDI
        } else if state.ier_msi() && state.msr_any_delta() {
            UART_IIR_MSI
        } else {
            UART_IIR_NO_INT
        };

        state.iir = new_irr | (state.iir & 0xF0);

        match new_irr {
            UART_IIR_NO_INT => self.interrupt_callback.lower(allocator),
            _ => self.interrupt_callback.raise(allocator),
        }
    }

    pub fn read(&self, allocator: &mut A, address: u8) -> Result<u8, ReadError> {
        let dlab = allocator.get(self.state).unwrap().dlab();
        let value = match address {
            0 if dlab => self.read_dll(allocator),
            0 => self.read_rbr(allocator),
            1 if dlab => self.read_dlh(allocator),
            1 => self.read_ier(allocator),
            2 => self.read_iir(allocator),
            3 => self.read_lcr(allocator),
            4 => return Err(ReadError::WriteOnly("Modem Control Register")),
            5 => self.read_lsr(allocator),
            6 => self.read_msr(allocator),
            _ => return Err(ReadError::AddressInvalid(address)),
        };
        Ok(value)
    }

    /// Same as [`Self::read`] but without performing side effects (i.e. no state is mutated).
    pub fn read_pure(&self, allocator: &A, address: u8) -> Result<u8, ReadError> {
        let dlab = allocator.get(self.state).unwrap().dlab();
        let value = match address {
            0 if dlab => self.read_dll_pure(allocator),
            0 => self.read_rbr_pure(allocator),
            1 if dlab => self.read_dlh_pure(allocator),
            1 => self.read_ier_pure(allocator),
            2 => self.read_iir_pure(allocator),
            3 => self.read_lcr_pure(allocator),
            4 => return Err(ReadError::WriteOnly("Modem Control Register")),
            5 => self.read_lsr_pure(allocator),
            6 => self.read_msr_pure(allocator),
            _ => return Err(ReadError::AddressInvalid(address)),
        };
        Ok(value)
    }

    pub fn write(&self, allocator: &mut A, address: u8, value: u8) -> Result<(), WriteError> {
        let dlab = allocator.get(self.state).unwrap().dlab();

        match address {
            0 if dlab => self.write_dll(allocator, value),
            0 => self.write_thr(allocator, value),
            1 if dlab => self.write_dlh(allocator, value),
            1 => self.write_ier(allocator, value),
            2 => self.write_fcr(allocator, value),
            3 => self.write_lcr(allocator, value),
            4 => self.write_mcr(allocator, value),
            5 => return Err(WriteError::ReadOnly("Line Status Register")),
            6 => return Err(WriteError::ReadOnly("Modem Status Register")),
            _ => return Err(WriteError::AddressInvalid(address)),
        }
        Ok(())
    }

    /// Reads the least significant (= low) byte of the Divisor Latch Register.
    pub fn read_dll(&self, allocator: &mut A) -> u8 {
        self.read_dll_pure(allocator)
    }

    /// Reads the least significant (= low) byte of the Divisor Latch Register without performing
    /// side effects.
    pub fn read_dll_pure(&self, allocator: &A) -> u8 {
        let dlr = allocator.get(self.state).unwrap().dlr;
        dlr as u8
    }

    /// Writes a value to the least significant (= low) byte of the Divisor Latch Register.
    pub fn write_dll(&self, allocator: &mut A, value: u8) {
        let dlr = &mut allocator.get_mut(self.state).unwrap().dlr;
        *dlr = (*dlr & 0xFF00) | value as u16
    }

    /// Reads the most significant (= high) byte of the Divisor Latch Register.
    pub fn read_dlh(&self, allocator: &mut A) -> u8 {
        self.read_dlh_pure(allocator)
    }

    /// Reads the most significant (= high) byte of the Divisor Latch Register without performing
    /// side effects.
    pub fn read_dlh_pure(&self, allocator: &A) -> u8 {
        let dlr = allocator.get(self.state).unwrap().dlr;
        (dlr >> 8) as u8
    }

    /// Writes a value to the most significant (= high) byte of the Divisor Latch Register.
    pub fn write_dlh(&self, allocator: &mut A, value: u8) {
        let dlr = &mut allocator.get_mut(self.state).unwrap().dlr;
        *dlr = ((value as u16) << 8) | (*dlr & 0xFF)
    }

    /// Reads the value of the Receiver Buffer Register.
    ///
    /// Returns an undefined value if the RX FIFO is empty.
    pub fn read_rbr(&self, allocator: &mut A) -> u8 {
        let state = allocator.get(self.state).unwrap();
        let value = state.rx_fifo_buf[0];
        if state.rx_fifo_len > 0 {
            let state = allocator.get_mut(self.state).unwrap();
            state
                .rx_fifo_buf
                .copy_within(1..(state.rx_fifo_len as usize), 0);
            state.rx_fifo_len -= 1;
            if state.rx_fifo_len == 0 {
                state.set_lsr_dr(false);
            }
        }
        self.update_interrupt(allocator);
        value
    }

    /// Reads the value of the Receiver Buffer Register without performing side effects.
    ///
    /// This is basically a peek operation.
    pub fn read_rbr_pure(&self, allocator: &A) -> u8 {
        let state = allocator.get(self.state).unwrap();
        state.rx_fifo_buf[0]
    }

    /// Writes a value to the Transmitter Holding Register.
    ///
    /// Discards the oldest value in the TX FIFO if it is full.
    pub fn write_thr(&self, allocator: &mut A, value: u8) {
        let state = allocator.get_mut(self.state).unwrap();
        if state.tx_fifo_len as usize == state.tx_fifo_buf.len() {
            state.tx_fifo_buf.copy_within(1.., 0);
            state.tx_fifo_len -= 1;
        }
        state.tx_fifo_buf[state.tx_fifo_len as usize] = value & state.char_mask();
        state.tx_fifo_len += 1;
        state.set_lsr_tfe(false);
        if state.tx_fifo_len as usize == state.tx_fifo_buf.len() {
            state.set_lsr_thre(false);
        }
    }

    /// Reads the value of the Interrupt Enable Register.
    pub fn read_ier(&self, allocator: &mut A) -> u8 {
        self.read_ier_pure(allocator)
    }

    /// Reads the value of the Interrupt Enable Register without performing side effects.
    pub fn read_ier_pure(&self, allocator: &A) -> u8 {
        allocator.get(self.state).unwrap().ier
    }

    /// Writes a value to the Interrupt Enable Register
    pub fn write_ier(&self, allocator: &mut A, value: u8) {
        allocator.get_mut(self.state).unwrap().ier = value;
        self.update_interrupt(allocator);
    }

    /// Reads the value of the Interrupt Identification Register.
    pub fn read_iir(&self, allocator: &mut A) -> u8 {
        self.read_iir_pure(allocator)
    }

    /// Reads the value of the Interrupt Identification Register without performing side effects.
    pub fn read_iir_pure(&self, allocator: &A) -> u8 {
        allocator.get(self.state).unwrap().iir
    }

    /// Writes a value to the FIFO Control Register.
    pub fn write_fcr(&self, allocator: &mut A, value: u8) {
        let state = allocator.get_mut(self.state).unwrap();
        let bits = value.view_bits::<Lsb0>();
        if bits[1] {
            state.rx_fifo_len = 0;
        }
        if bits[2] {
            state.tx_fifo_len = 0;
        }
        // TODO: actually match on 0b00, 0b01, 0b10, 0b11 rather than this ugly bool mess
        state.rx_fifo_itl = match (bits[7], bits[6]) {
            (false, false) => 1,
            (false, true) => 4,
            (true, false) => 8,
            (true, true) => 14,
        };
        self.update_interrupt(allocator);
    }

    /// Reads the value of the Line Control Register.
    pub fn read_lcr(&self, allocator: &mut A) -> u8 {
        self.read_lcr_pure(allocator)
    }

    /// Reads the value of the Line Control Register without performing side effects.
    pub fn read_lcr_pure(&self, allocator: &A) -> u8 {
        allocator.get(self.state).unwrap().lcr
    }

    /// Writes a value to the Line Control Register.
    pub fn write_lcr(&self, allocator: &mut A, value: u8) {
        allocator.get_mut(self.state).unwrap().lcr = value;
    }

    /// Writes a value to the Modem Control Register.
    pub fn write_mcr(&self, allocator: &mut A, value: u8) {
        // Nothing needs to be done, as the scenario of an attached "modem" is not simulated,
        // and the MCR has only write-only fields.
        let _ = (allocator, value);
    }

    /// Reads the value of the Line Status Register.
    pub fn read_lsr(&self, allocator: &mut A) -> u8 {
        let state = allocator.get(self.state).unwrap();
        let value = state.lsr;
        // The Overrun Error indicator is cleared when reading the Line Status Register
        if state.lsr_oe() {
            allocator.get_mut(self.state).unwrap().set_lsr_oe(false);
        }
        value
    }

    /// Reads the value of the Line Status Register without performing side effects.
    pub fn read_lsr_pure(&self, allocator: &A) -> u8 {
        let state = allocator.get(self.state).unwrap();
        state.lsr
    }

    /// Reads the value of the Modem Status Register.
    pub fn read_msr(&self, allocator: &mut A) -> u8 {
        self.read_msr_pure(allocator)
    }

    /// Reads the value of the Modem Status Register without performing side effects.
    pub fn read_msr_pure(&self, allocator: &A) -> u8 {
        allocator.get(self.state).unwrap().msr
    }

    pub fn drop(self, allocator: &mut A) {
        allocator.remove(self.state).unwrap()
    }
}

// Public methods mend to be used by an external consumer of this library
impl<A: Allocator> Uart<A> {
    /// Amount of bytes that can be read from the UART.
    pub fn pending_output_amount(&self, allocator: &A) -> usize {
        let state = allocator.get(self.state).unwrap();
        if !state.is_operational() {
            return 0;
        }
        state.tx_fifo_len as usize
    }

    /// Amount of bytes that can be written to the UART.
    pub fn input_space(&self, allocator: &A) -> usize {
        let state = allocator.get(self.state).unwrap();
        if !state.is_operational() {
            return 0;
        }
        state.rx_fifo_buf.len() - state.rx_fifo_len as usize
    }

    /// Writes up to [`Self::pending_output_amount`] bytes from `input` into the rx buffer, and
    /// returns [`Self::pending_output_amount`] bytes of output.
    ///
    /// The amount of bytes read from `input` is also returned.
    ///
    /// This function is expensive to execute, even if nothing is read or written, so make sure to
    /// check [`Self::pending_output_amount`] and [`Self::pending_output_amount`] before calling
    /// this function to make sure it makes sense to do so.
    pub fn push_and_read(&self, allocator: &mut A, input: &[u8]) -> (usize, Vec<u8>) {
        let state = allocator.get_mut(self.state).unwrap();
        if !state.is_operational() {
            return (0, Vec::new());
        }

        let rx_buf = &mut state.rx_fifo_buf[(state.rx_fifo_len as usize)..];
        let input_size = rx_buf.len().min(input.len());
        rx_buf[..input_size].copy_from_slice(&input[..input_size]);
        state.rx_fifo_len += input_size as u8;
        if state.rx_fifo_len > 0 {
            state.set_lsr_dr(true);
        }

        let output_size = state.tx_fifo_len as usize;
        let output = state.tx_fifo_buf[..output_size].to_owned();

        state.tx_fifo_len = 0;

        state.set_lsr_thre(true);
        state.set_lsr_tfe(true);

        self.update_interrupt(allocator);

        (input_size, output)
    }
}

impl<A: Allocator> Bus<A> for Uart<A> {
    /// See [`Bus::read`].
    ///
    /// The address space is circular 8-bit.
    ///
    /// Only the first byte (if `buf.len() >= 1`) will be updated. Invalid reads will cause that
    /// first byte to have an undefined value. The other bytes are always left untouched.
    fn read(&self, buf: &mut [u8], allocator: &mut A, address: u32) {
        match self.read(allocator, address as u8) {
            Ok(value) => {
                if let Some(out) = buf.get_mut(0) {
                    *out = value
                }
            }
            Err(ReadError::AddressInvalid(_)) => {}
            Err(ReadError::WriteOnly(_)) => {}
        }
    }

    /// See [`Bus::read_debug`].
    ///
    /// The address space is circular 8-bit.
    ///
    /// Only the first byte (if `buf.len() >= 1`) will be updated. Invalid reads will cause that
    /// first byte to have an undefined value. The other bytes are always left untouched.
    fn read_debug(&self, buf: &mut [u8], allocator: &A, address: u32) {
        match self.read_pure(allocator, address as u8) {
            Ok(value) => {
                if let Some(out) = buf.get_mut(0) {
                    *out = value
                }
            }
            Err(ReadError::AddressInvalid(_)) => {}
            Err(ReadError::WriteOnly(_)) => {}
        }
    }

    /// See [`Bus::write`].
    ///
    /// The address space is circular 8-bit.
    ///
    /// Only the first byte (if `buf.len() >= 1`) is written.
    /// In case `buf.len() == 0`, the value `0x00` is used.
    fn write(&self, allocator: &mut A, address: u32, buf: &[u8]) {
        let _ = self.write(allocator, address as u8, buf.first().copied().unwrap_or(0));
    }
}
