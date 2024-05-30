use std::collections::HashSet;

use crate::Simulator;

#[derive(Debug, Clone)]
pub enum Event {
    DoneStep,
    PoweredDown,
    Break,
    ReachedStart,
}

#[derive(Debug)]
pub enum RunEvent {
    Event(Event),
    IncomingData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionMode {
    Step,
    StepBack,
    RangeStep(u32, u32),
    Continue,
    ReverseContinue,
}

#[derive(Debug)]
pub struct SimTarget {
    pub simulator: Simulator,
    pub breakpoints: HashSet<u32>,
    pub execution_mode: ExecutionMode,
}

impl SimTarget {
    pub fn new(simulator: Simulator) -> Self {
        Self {
            simulator,
            breakpoints: HashSet::new(),
            execution_mode: ExecutionMode::Continue,
        }
    }

    pub fn _reset(&mut self) {
        self.simulator.step_with("reset board", |allocator, board| {
            board.reset(allocator);
        });
    }

    pub fn step(&mut self) -> Option<Event> {
        if !self.simulator.redo_step() {
            self.simulator.step();
        }
        let (allocator, board) = self.simulator.inspect();
        if self
            .breakpoints
            .contains(&board.core().registers(allocator).pc())
        {
            return Some(Event::Break);
        }
        if board.is_powered_down(allocator) {
            return Some(Event::PoweredDown);
        }
        None
    }

    pub fn step_back(&mut self) -> Option<Event> {
        if !self.simulator.undo_step() {
            return Some(Event::ReachedStart);
        }
        let (allocator, board) = self.simulator.inspect();
        if self
            .breakpoints
            .contains(&board.core().registers(allocator).pc())
        {
            return Some(Event::Break);
        }
        if board.is_powered_down(allocator) {
            return Some(Event::PoweredDown);
        }
        None
    }

    pub fn run(&mut self, mut poll_incoming_data: impl FnMut() -> bool) -> RunEvent {
        match self.execution_mode {
            ExecutionMode::Step => RunEvent::Event(self.step().unwrap_or(Event::DoneStep)),
            ExecutionMode::StepBack => RunEvent::Event(self.step_back().unwrap_or(Event::DoneStep)),
            ExecutionMode::Continue => {
                let mut cycles = 0;
                loop {
                    if cycles % 1024 == 0 {
                        // poll for incoming data
                        if poll_incoming_data() {
                            break RunEvent::IncomingData;
                        }
                    }
                    cycles += 1;

                    if let Some(event) = self.step() {
                        break RunEvent::Event(event);
                    };
                }
            }
            ExecutionMode::RangeStep(start, end) => {
                let mut cycles = 0;
                loop {
                    if cycles % 1024 == 0 {
                        // poll for incoming data
                        if poll_incoming_data() {
                            break RunEvent::IncomingData;
                        }
                    }
                    cycles += 1;

                    if let Some(event) = self.step() {
                        break RunEvent::Event(event);
                    };

                    let (allocator, board) = self.simulator.inspect();

                    if !(start..end).contains(&board.core().registers(allocator).pc()) {
                        break RunEvent::Event(Event::DoneStep);
                    }
                }
            }
            ExecutionMode::ReverseContinue => {
                let mut cycles = 0;
                loop {
                    if cycles % 64 == 0 {
                        // poll for incoming data
                        if poll_incoming_data() {
                            break RunEvent::IncomingData;
                        }
                    }
                    cycles += 1;

                    if let Some(event) = self.step_back() {
                        break RunEvent::Event(event);
                    };
                }
            }
        }
    }
}
