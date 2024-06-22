use std::{
    collections::HashSet,
    io::{stdout, Write},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{poll, read},
    style::Print,
    terminal::Clear,
    QueueableCommand,
};
use red_planet_core::{simulator::SimulationAllocator, Allocator, ArrayAccessor, ArrayAccessorMut};

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

    pub output_buffer: <SimulationAllocator as Allocator>::ArrayId<u8>,
    pub output_buffer_len: <SimulationAllocator as Allocator>::Id<usize>,

    pub last_output: Vec<u8>,
}

fn read_from_term(buf: &mut [u8]) -> std::io::Result<usize> {
    let mut size = 0;
    loop {
        if poll(Duration::from_millis(0))? {
            // It's guaranteed that the `read()` won't block when the `poll()`
            // function returns `true`
            if let crossterm::event::Event::Key(event) = read()? {
                match event.code {
                    crossterm::event::KeyCode::Char(c) if c.is_ascii() => {
                        let writen = (&mut buf[size..]).write(&[c as u8]).unwrap();
                        size += writen;
                        if writen == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        } else {
            break;
        }
    }

    Ok(size)
}

impl SimTarget {
    pub fn new(mut simulator: Simulator) -> Self {
        let (output_buffer, output_buffer_len) =
            simulator.step_with("adding output buffer", |allocator, _| {
                let output_buffer = allocator.insert_array(0, 1024);
                let output_buffer_len = allocator.insert(0);
                (output_buffer, output_buffer_len)
            });

        Self {
            simulator,
            breakpoints: HashSet::new(),
            execution_mode: ExecutionMode::Continue,
            output_buffer,
            output_buffer_len,
            last_output: Vec::new(),
        }
    }

    pub fn _reset(&mut self) {
        self.simulator.step_with("reset board", |allocator, board| {
            board.reset(allocator);
        });
    }

    pub fn write_to_output(&mut self) {
        let (allocator, _) = self.simulator.inspect();
        let Ok(len) = allocator.get(self.output_buffer_len) else {
            return;
        };
        let output_buffer = allocator.get_array(self.output_buffer).unwrap();

        let mut buf = vec![0; *len];
        let _ = output_buffer.read(&mut buf, 0);

        if self.last_output == buf {
            return;
        }

        self.last_output = buf;

        let mut stdout = stdout();
        stdout.queue(cursor::MoveTo(0, 0)).ok();
        stdout
            .queue(Print(
                String::from_utf8_lossy(&self.last_output).replace('\n', "\r\n"),
            ))
            .ok();
        stdout
            .queue(Clear(crossterm::terminal::ClearType::FromCursorDown))
            .ok();

        stdout.flush().ok();
    }

    pub fn com_with_uart(&mut self) {
        let (allocator, board) = self.simulator.inspect();

        let pending_output_amount = board.uart0().pending_output_amount(allocator);
        let input_space = board.uart0().input_space(allocator);

        let input_buf = if input_space != 0 {
            let mut buf = [0; 16];
            let Ok(input_read) = read_from_term(&mut buf[..input_space]) else {
                return;
            };
            buf[..input_read].to_owned()
        } else {
            Vec::new()
        };

        if !input_buf.is_empty() || pending_output_amount != 0 {
            let output_buffer = self.output_buffer;
            let output_buffer_len = self.output_buffer_len;

            self.simulator
                .step_with("com with uart", move |allocator, board| {
                    let output = board.uart0().push_and_read(allocator, &input_buf).1;

                    let Ok(len) = allocator.get(output_buffer_len) else {
                        return;
                    };
                    let len = *len;

                    let _ = allocator
                        .get_array_mut(output_buffer)
                        .unwrap()
                        .write(len, &output);

                    *allocator.get_mut(output_buffer_len).unwrap() += output.len();
                });
        }
    }

    pub fn step(&mut self) -> Option<Event> {
        if !self.simulator.redo_step() {
            self.com_with_uart();
            self.simulator.step();
        }
        self.write_to_output();

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
        self.write_to_output();

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
