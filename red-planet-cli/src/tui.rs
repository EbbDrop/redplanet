use std::{io::stdout, time::Duration};

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures::{FutureExt, StreamExt};
use log::{error, info};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Gauge},
    Frame, Terminal,
};
use red_planet_core::registers::{Registers, Specifier};
use tokio::{
    select, spawn,
    sync::{mpsc::UnboundedSender, watch},
    time::interval,
};
use tui_logger::{TuiLoggerWidget, TuiWidgetState};
use tui_textarea::TextArea;

use crate::target::{command::Command, ExecutionType, SharedTargetState};

/// Sets up the terminal on creation, and resets it back when dropped.
pub struct TermSetupDropGard {}

impl TermSetupDropGard {
    pub fn new() -> std::io::Result<Self> {
        std::io::stdout().execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        Ok(Self {})
    }
}

impl Drop for TermSetupDropGard {
    fn drop(&mut self) {
        // Ignore all errors on drop, resting the terminal is on a best effort basis
        let _ = std::io::stdout().execute(LeaveAlternateScreen);
        let _ = std::io::stdout().execute(crossterm::cursor::Show);
        let _ = disable_raw_mode();
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum Selected {
    Uart,
    #[default]
    Prompt,
}

pub struct TuiState {
    command_sender: UnboundedSender<Command>,
    uart_sender: UnboundedSender<u8>,

    shared_state: watch::Receiver<SharedTargetState>,

    log_widget: TuiWidgetState,
    prompt: TextArea<'static>,

    selected: Selected,
    last_command: Option<String>,
}

impl TuiState {
    pub fn new(
        command_sender: UnboundedSender<Command>,
        shared_state: watch::Receiver<SharedTargetState>,
        uart_sender: UnboundedSender<u8>,
    ) -> Self {
        let mut prompt_widget = TextArea::default();
        prompt_widget.set_cursor_line_style(Style::default());

        Self {
            command_sender,
            shared_state,
            uart_sender,

            log_widget: TuiWidgetState::new().set_default_display_level(log::LevelFilter::Trace),
            prompt: prompt_widget,

            selected: Selected::default(),
            last_command: None,
        }
    }

    fn run_command(&mut self, command_str: String) -> bool {
        let mut command_str = command_str.trim();
        if command_str.is_empty() {
            if let Some(old_command) = &self.last_command {
                command_str = old_command.as_str();
            }
        }

        enum CommandResponse {
            Registers(oneshot::Receiver<Registers>),
        }

        let (command, command_response) = match command_str.trim() {
            "q" | "quit" => (Command::Exit, None),
            "p" | "pause" => (Command::Pause, None),
            "c" | "continue" => (Command::Continue, None),
            "s" | "step" => (Command::Step, None),
            "rc" | "reverse-continue" => (Command::ReverseContinue, None),
            "rs" | "reverse-step" => (Command::StepBack, None),
            "regs" => {
                let (sender, receiver) = oneshot::channel();
                (
                    Command::ReadRegisters(sender),
                    Some(CommandResponse::Registers(receiver)),
                )
            }
            _ => return false,
        };
        self.last_command = Some(command_str.to_owned());
        if let Err(e) = self.command_sender.send(command) {
            error!("Failed to send command: {}", e.0);
        }

        if let Some(command_response) = command_response {
            spawn(async move {
                match command_response {
                    CommandResponse::Registers(registers) => {
                        if let Ok(registers) = registers.await {
                            for r in Specifier::iter_all() {
                                info!("${}: {}", r, registers.x(r));
                            }
                            info!("$pc: {}", registers.pc());
                        }
                    }
                }
            });
        }

        true
    }

    fn draw_status(state: &SharedTargetState, frame: &mut Frame, rect: Rect) {
        let ratio = match state.total_steps {
            0 => 0.0,
            total_steps => state.current_step as f64 / total_steps as f64,
        }
        .clamp(0.0, 1.0);

        let running_state_name = match state.state {
            Some(ExecutionType::Step) => "Step",
            Some(ExecutionType::StepBack) => "Step Back",
            Some(ExecutionType::RangeStep(_, _)) => "Running",
            Some(ExecutionType::Continue) => "Running",
            Some(ExecutionType::ReverseContinue) => "Running Back",
            None => "Stopped",
        };

        let state_block = Block::bordered().title("State");

        let [running_state_area, current_step_area, bar_area, total_steps_area] =
            Layout::horizontal([
                Constraint::Length(12),
                Constraint::Length(9),
                Constraint::Fill(1),
                Constraint::Length(9),
            ])
            .spacing(1)
            .areas(state_block.inner(rect));

        let state_bar = Gauge::default()
            .gauge_style(Style::default().fg(Color::Blue))
            .label("")
            .use_unicode(true)
            .ratio(ratio);

        let running_state = Span::raw(running_state_name);
        let current_step = Span::styled(
            state.current_step.to_string(),
            Style::default().fg(Color::Blue),
        );
        let total_steps = Span::raw(state.total_steps.to_string());

        frame.render_widget(state_block, rect);
        frame.render_widget(running_state, running_state_area);

        frame.render_widget(current_step, current_step_area);
        frame.render_widget(state_bar, bar_area);
        frame.render_widget(total_steps, total_steps_area);
    }

    fn draw(&mut self, frame: &mut Frame) {
        let shared_state = self.shared_state.borrow_and_update();
        let uart_output = String::from_utf8_lossy(&shared_state.output_buffer);

        let [app_area, log_area] =
            Layout::horizontal(Constraint::from_percentages([70, 30])).areas(frame.size());

        let [status_area, uart_area, prompt_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .areas(app_area);

        Self::draw_status(&shared_state, frame, status_area);

        let selected_style = Style::default().fg(Color::Green);
        let deselected_style = Style::default();
        let (uart_style, prompt_style) = match &self.selected {
            Selected::Uart => (selected_style, deselected_style),
            Selected::Prompt => (deselected_style, selected_style),
        };
        let uart = ratatui::widgets::Paragraph::new(uart_output).block(
            Block::new()
                .borders(Borders::ALL)
                .title("UART")
                .border_style(uart_style),
        );

        self.prompt.set_block(
            Block::new()
                .borders(Borders::ALL)
                .title("Command")
                .border_style(prompt_style),
        );

        let log = TuiLoggerWidget::default()
            .output_separator('|')
            .output_timestamp(None)
            .output_level(None)
            .output_target(false)
            .output_file(false)
            .output_line(false)
            .style_error(Style::default().fg(Color::Red))
            .style_debug(Style::default().fg(Color::Green))
            .style_warn(Style::default().fg(Color::Yellow))
            .style_trace(Style::default().fg(Color::Magenta))
            .style_info(Style::default().fg(Color::Cyan))
            .block(Block::new().borders(Borders::ALL).title("Log"))
            .state(&self.log_widget);

        frame.render_widget(uart, uart_area);
        frame.render_widget(self.prompt.widget(), prompt_area);
        frame.render_widget(log, log_area)
    }

    fn handle_event(&mut self, event: Event) {
        log::trace!("Got cli event: {event:?}");
        if let Event::Key(k) = event {
            match &k.code {
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
                    self.selected = match &self.selected {
                        Selected::Uart => Selected::Prompt,
                        Selected::Prompt => Selected::Uart,
                    };
                }
                KeyCode::Char(c) => {
                    if *c == 'c' && k.modifiers.contains(KeyModifiers::CONTROL) {
                        info!("Pauzing simulation, use `quit` to exit");
                        let _ = self.command_sender.send(Command::Pause);
                        return;
                    }
                    match &self.selected {
                        Selected::Uart => {
                            for byte in c.encode_utf8(&mut [0; 4]).as_bytes() {
                                let _ = self.uart_sender.send(*byte);
                            }
                        }
                        Selected::Prompt => {
                            self.prompt.input(event);
                        }
                    }
                }
                KeyCode::Backspace => match &self.selected {
                    Selected::Uart => {
                        let _ = self.uart_sender.send(0x08);
                    }
                    Selected::Prompt => {
                        self.prompt.input(event);
                    }
                },
                KeyCode::Enter => match &self.selected {
                    Selected::Uart => {
                        let _ = self.uart_sender.send(b'\n');
                    }
                    Selected::Prompt => {
                        let command = self.prompt.lines()[0].to_owned();
                        if self.run_command(command) {
                            self.prompt.move_cursor(tui_textarea::CursorMove::End);
                            self.prompt.delete_line_by_head();
                        }
                    }
                },
                _ => {}
            }
        }
    }

    /// Will bock until the user exits
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) {
        let mut event_stream = EventStream::new();
        let mut interval = interval(Duration::from_secs_f32(1.0 / 60.0));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            terminal.draw(|frame| self.draw(frame)).unwrap();

            let event = event_stream.next().fuse();
            select! {
                event = event => match event {
                    Some(Ok(event)) => self.handle_event(event),
                    Some(Err(e)) => {
                        error!("Failed to read from keyboard: {e}");
                        break;
                    }
                    None => {
                        error!("Event stream closed unexpectedly");
                        break;
                    }
                },
                _ = interval.tick() => {
                    // update every 1 / 60 secconds
                }
            }
        }
        let _ = self.command_sender.send(Command::Exit);
    }
}

pub async fn run_tui(
    command_sender: UnboundedSender<Command>,
    shared_state_receiver: watch::Receiver<SharedTargetState>,
    uart_sender: UnboundedSender<u8>,
) {
    let mut tui = TuiState::new(command_sender, shared_state_receiver, uart_sender);
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout())).unwrap();
    tui.run(&mut terminal).await
}
