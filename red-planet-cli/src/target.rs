pub mod command;

use std::collections::HashSet;

use command::Command;
use gdbstub::target::TargetError;
use gdbstub_arch::riscv::reg::id::RiscvRegId;
use log::{error, info, trace};
use red_planet_core::{
    board::Board,
    registers::Specifier,
    simulator::{SimulationAllocator, UndoStepStopReason},
    Allocator, ArrayAccessor, ArrayAccessorMut,
};
use tokio::sync::{
    mpsc::{error::TryRecvError, unbounded_channel, UnboundedReceiver, UnboundedSender},
    watch,
};

use crate::Simulator;

#[derive(Debug, Clone)]
pub enum Event {
    DoneStep,
    PoweredDown,
    Break,
    ReachedStart,
    Pause,
}

#[derive(Debug)]
pub enum AdvanceResult {
    Event(Event),
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionType {
    Step,
    StepBack,
    RangeStep(u32, u32),
    Continue,
    ReverseContinue,
}

#[derive(Debug, Default)]
pub struct SharedTargetState {
    pub output_buffer: Vec<u8>,
    pub total_steps: usize,
    pub current_step: usize,
    pub state: Option<ExecutionType>,
}

pub struct SimTarget {
    command_channel: UnboundedReceiver<Command>,
    event_channel: UnboundedSender<Event>,
    uart_channel: UnboundedReceiver<u8>,

    break_reasons: BreakReasons,

    state: TargetState,

    shared_state: watch::Sender<SharedTargetState>,
}

#[derive(Debug, Default)]
struct BreakReasons {
    breakpoints: HashSet<u32>,
}

impl BreakReasons {
    fn should_break(
        &self,
        allocator: &SimulationAllocator,
        board: &Board<SimulationAllocator>,
    ) -> bool {
        self.breakpoints
            .contains(&board.core().registers(allocator).pc())
    }
}

struct TargetState {
    output_buffer: <SimulationAllocator as Allocator>::ArrayId<u8>,
    output_buffer_len: <SimulationAllocator as Allocator>::Id<usize>,

    execution_type: Option<ExecutionType>,
}

impl TargetState {
    fn read_uart_output(&self, simulator: &Simulator, buf: &mut Vec<u8>) {
        let allocator = simulator.allocator();
        let Ok(len) = allocator.get(self.output_buffer_len) else {
            // Went all the way to the start, clearing buffer
            buf.clear();
            return;
        };
        let output_buffer = allocator.get_array(self.output_buffer).unwrap();

        buf.resize(*len, 0);

        if !output_buffer.read(buf.as_mut_slice(), 0) {
            error!("Failed to read into output buffer");
        }
    }

    fn write_to_shared_state(
        &mut self,
        simulator: &Simulator,
        shared_state: &watch::Sender<SharedTargetState>,
    ) {
        shared_state.send_modify(|shared_state| {
            self.read_uart_output(simulator, &mut shared_state.output_buffer);

            shared_state.total_steps = simulator.available_steps();
            shared_state.current_step = simulator.current_steps();

            shared_state.state = self.execution_type;
        })
    }
}

impl SimTarget {
    pub fn new(
        simulator: &mut Simulator,
        shared_state: watch::Sender<SharedTargetState>,
        uart_channel: UnboundedReceiver<u8>,
    ) -> (Self, UnboundedSender<Command>, UnboundedReceiver<Event>) {
        let (output_buffer, output_buffer_len) =
            simulator.step_with("adding output buffer", |allocator, _| {
                let output_buffer = allocator.insert_array(0, 1024);
                let output_buffer_len = allocator.insert(0);
                (output_buffer, output_buffer_len)
            });

        let (c_sender, c_receiver) = unbounded_channel();
        let (e_sender, e_receiver) = unbounded_channel();

        let target = Self {
            command_channel: c_receiver,
            event_channel: e_sender,
            uart_channel,

            shared_state,

            break_reasons: BreakReasons::default(),

            state: TargetState {
                output_buffer,
                output_buffer_len,

                execution_type: None,
            },
        };
        (target, c_sender, e_receiver)
    }

    fn com_with_uart(&mut self, simulator: &mut Simulator) {
        let (allocator, board) = simulator.inspect();

        let pending_output_amount = board.uart0().pending_output_amount(allocator);
        let input_space = board.uart0().input_space(allocator);

        let input_buf = if input_space != 0 {
            let mut input_buf = Vec::new();
            while let Ok(byte) = self.uart_channel.try_recv() {
                input_buf.push(byte);
                if input_buf.len() >= input_space {
                    break;
                }
            }
            input_buf
        } else {
            Vec::new()
        };

        if !input_buf.is_empty() || pending_output_amount != 0 {
            let output_buffer = self.state.output_buffer;
            let output_buffer_len = self.state.output_buffer_len;

            simulator.step_with("com with uart", move |allocator, board| {
                let output = board.uart0().push_and_read(allocator, &input_buf).1;

                let Ok(len) = allocator.get(output_buffer_len) else {
                    return;
                };
                let len = *len;

                trace!(
                    "Updating output buffer to {}+{}={} bytes",
                    len,
                    output.len(),
                    len + output.len()
                );
                let _ = allocator
                    .get_array_mut(output_buffer)
                    .unwrap()
                    .write(len, &output);

                *allocator.get_mut(output_buffer_len).unwrap() += output.len();
            });
        }
    }

    fn step(&mut self, simulator: &mut Simulator) -> Option<Event> {
        if !simulator.redo_step() {
            self.com_with_uart(simulator);
            simulator.step();
        }

        let (allocator, board) = simulator.inspect();

        if self.break_reasons.should_break(allocator, board) {
            return Some(Event::Break);
        }
        if board.is_powered_down(allocator) {
            return Some(Event::PoweredDown);
        }
        None
    }

    fn step_back(&mut self, simulator: &mut Simulator) -> Option<Event> {
        if !simulator.undo_step() {
            return Some(Event::ReachedStart);
        }

        let (allocator, board) = simulator.inspect();
        if self.break_reasons.should_break(allocator, board) {
            return Some(Event::Break);
        }
        if board.is_powered_down(allocator) {
            return Some(Event::PoweredDown);
        }
        None
    }

    fn advance_sim(
        &mut self,
        execution_type: ExecutionType,
        simulator: &mut Simulator,
    ) -> AdvanceResult {
        match execution_type {
            ExecutionType::Step => {
                AdvanceResult::Event(self.step(simulator).unwrap_or(Event::DoneStep))
            }
            ExecutionType::StepBack => {
                AdvanceResult::Event(self.step_back(simulator).unwrap_or(Event::DoneStep))
            }
            ExecutionType::Continue => {
                for _ in 0..1024 {
                    if let Some(event) = self.step(simulator) {
                        return AdvanceResult::Event(event);
                    };
                }
                AdvanceResult::Continue
            }
            ExecutionType::RangeStep(start, end) => {
                for _ in 0..1024 {
                    if let Some(event) = self.step(simulator) {
                        return AdvanceResult::Event(event);
                    };

                    let (allocator, board) = simulator.inspect();

                    if !(start..end).contains(&board.core().registers(allocator).pc()) {
                        return AdvanceResult::Event(Event::DoneStep);
                    }
                }
                AdvanceResult::Continue
            }
            ExecutionType::ReverseContinue => {
                // Using this var to only send a single true about the command channel, this way
                // we can go as far back as possible if a command where to accrue.
                let mut has_notified_about_command_channel = false;

                let result = simulator.undo_steps_until(
                    |allocator, board| {
                        if self.break_reasons.should_break(allocator, board) {
                            return Some(AdvanceResult::Event(Event::Break));
                        }

                        if !self.command_channel.is_empty() && !has_notified_about_command_channel {
                            has_notified_about_command_channel = true;
                            return Some(AdvanceResult::Continue);
                        }

                        None
                    },
                    |simulator| {
                        self.state
                            .write_to_shared_state(simulator, &self.shared_state);
                    },
                );

                match result {
                    UndoStepStopReason::ReachedStart => AdvanceResult::Event(Event::ReachedStart),
                    UndoStepStopReason::Pred(result) => result,
                }
            }
        }
    }

    fn read_register(&self, reg_id: RiscvRegId<u32>, simulator: &mut Simulator) -> Option<u32> {
        let (allocator, board) = simulator.inspect();

        match reg_id {
            RiscvRegId::Gpr(i) => {
                let registers = board.core().registers(allocator);
                Some(registers.x(Specifier::new(i).unwrap()))
            }
            RiscvRegId::Fpr(_) => None,
            RiscvRegId::Pc => Some(board.core().registers(allocator).pc()),
            RiscvRegId::Csr(specifier) => simulator
                .step_with("inspect csr", move |allocator, board| {
                    board.core().read_csr(
                        allocator,
                        specifier,
                        board.core().privilege_mode(allocator),
                    )
                })
                .ok(),
            RiscvRegId::Priv => Some(board.core().privilege_mode(allocator) as u8 as u32),
            _ => None,
        }
    }

    pub fn execute_command(&mut self, command: Command, simulator: &mut Simulator) -> bool {
        trace!("Got command: {}", &command);
        match command {
            Command::Exit => return true,
            Command::Pause => {
                self.state.execution_type = None;
                let _ = self.event_channel.send(Event::Pause);
            }
            Command::Continue => self.state.execution_type = Some(ExecutionType::Continue),
            Command::ReverseContinue => {
                self.state.execution_type = Some(ExecutionType::ReverseContinue)
            }
            Command::Step => self.state.execution_type = Some(ExecutionType::Step),
            Command::StepBack => self.state.execution_type = Some(ExecutionType::StepBack),
            Command::RangeStep(s, e) => {
                self.state.execution_type = Some(ExecutionType::RangeStep(s, e))
            }
            Command::AddBreakpoint(addr) => {
                self.break_reasons.breakpoints.insert(addr);
            }
            Command::RemoveBreakpoint(addr) => {
                self.break_reasons.breakpoints.remove(&addr);
            }
            Command::ReadRegisters(return_channel) => {
                let (allocator, board) = simulator.inspect();

                let registers = board.core().registers(allocator);

                let _ = return_channel.send(registers.clone());
            }
            Command::WriteRegisters(registers) => {
                simulator.step_with("write all registers", move |allocator, board| {
                    *board.core().registers_mut(allocator) = registers.clone();
                })
            }
            Command::ReadRegister(register, return_channel) => {
                if let Some(value) = self.read_register(register, simulator) {
                    let _ = return_channel.send(value);
                }
            }
            Command::ReadAddrs(addr, len, return_channel) => {
                let (allocator, board) = simulator.inspect();

                let memory = board.core().mmu();

                let mut data = vec![0; len];
                let result = match memory.read_range_debug(&mut data, allocator, addr) {
                    Ok(()) => Ok(data),
                    Err(_) => Err(TargetError::NonFatal),
                };
                let _ = return_channel.send(result);
            }
            Command::WriteAddrs(addr, data, return_channel) => {
                let result = simulator.step_with("write data", move |allocator, board| {
                    let memory = board.core().mmu();
                    memory.write_range(allocator, addr, &data)
                });
                let _ = return_channel.send(result);
            }
            Command::DeleteFuture => {
                simulator.clear_forward_history();
            }
            Command::GoTo(steps) => {
                if simulator.available_steps() >= steps {
                    simulator.go_to(steps);
                }
            }
        }
        false
    }

    pub async fn run(mut self, mut simulator: Simulator) {
        loop {
            self.state
                .write_to_shared_state(&simulator, &self.shared_state);
            let Some(execution_type) = &self.state.execution_type else {
                match self.command_channel.recv().await {
                    Some(command) => {
                        if self.execute_command(command, &mut simulator) {
                            break;
                        }
                    }
                    None => {
                        info!("All command channels droped, stoping target");
                        break;
                    }
                }
                continue;
            };

            let result = self.advance_sim(*execution_type, &mut simulator);
            match result {
                AdvanceResult::Event(e) => {
                    log::info!("Target stoped due to {:?}", e);
                    self.state.execution_type = None;
                    let _ = self.event_channel.send(e);
                }
                AdvanceResult::Continue => {
                    // Check if there have been any command in the meantime.
                    let command = self.command_channel.try_recv();
                    match command {
                        Ok(command) => {
                            if self.execute_command(command, &mut simulator) {
                                break;
                            }
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => {
                            info!("All command channels droped, stoping target");
                            break;
                        }
                    }
                }
            }
        }
    }
}
