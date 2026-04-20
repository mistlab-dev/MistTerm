use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;
use tui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Tabs, List, ListItem},
    Terminal,
};

#[derive(Parser, Debug)]
#[clap(name = "MistTerm", author, version, about = "🌫️ MistTerm - A Rust terminal emulator with rzsz integration")]
struct Args {
    /// Connect to remote host via SSH
    #[clap(short, long)]
    host: Option<String>,

    /// Port for SSH connection
    #[clap(short, long, default_value = "22")]
    port: u16,

    /// Username for SSH connection
    #[clap(short, long)]
    user: Option<String>,

    /// Serial port device
    #[clap(short = 'D', long)]
    device: Option<String>,

    /// Baud rate for serial connection
    #[clap(short, long, default_value = "115200")]
    baud: u32,
}

#[derive(Debug)]
enum AppMode {
    Terminal,
    RzTransfer { filename: String },
    SzTransfer { filename: String },
}

struct App {
    mode: AppMode,
    tab_index: usize,
    input_buffer: String,
    output_lines: Vec<String>,
    status_message: String,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            mode: AppMode::Terminal,
            tab_index: 0,
            input_buffer: String::new(),
            output_lines: vec![
                "🌫️ MistTerm v0.1.0".to_string(),
                "Rust terminal emulator with rzsz integration".to_string(),
                "".to_string(),
                "Press Ctrl+T for command palette".to_string(),
                "Press Tab to switch views".to_string(),
                "".to_string(),
                "Ready.".to_string(),
            ],
            status_message: "Terminal mode | Ctrl+Q to quit".to_string(),
            should_quit: false,
        }
    }

    fn handle_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            match key.code {
                // Quit
                KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true;
                }
                // Command palette trigger
                KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.output_lines.push("Command: rz, sz, clear, help, quit".to_string());
                    self.status_message = "Command mode".to_string();
                }
                // Tab switching
                KeyCode::Tab => {
                    self.tab_index = (self.tab_index + 1) % 3;
                }
                // Enter
                KeyCode::Enter => {
                    let input = self.input_buffer.clone();
                    if !input.is_empty() {
                        self.output_lines.push(format!("> {}", input));
                        self.process_command(&input);
                        self.input_buffer.clear();
                    }
                }
                // Backspace
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                // Regular character input
                KeyCode::Char(c) => {
                    self.input_buffer.push(c);
                }
                _ => {}
            }
        }
    }

    fn process_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        match parts.first().map(|s| s.to_lowercase()).as_deref() {
            Some("rz") => {
                self.output_lines.push("📁 Receiving file... (rz mode)".to_string());
                self.output_lines.push("  Waiting for sender (use sz on remote)...".to_string());
                self.status_message = "rz: Receiving file...".to_string();
            }
            Some("sz") => {
                if parts.len() > 1 {
                    self.output_lines.push(format!("📤 Sending file: {} (sz mode)", parts[1]));
                    self.status_message = format!("sz: Sending {}...", parts[1]);
                } else {
                    self.output_lines.push("Usage: sz <filename>".to_string());
                }
            }
            Some("clear") => {
                self.output_lines.clear();
            }
            Some("help") => {
                self.output_lines.push("MistTerm Commands:".to_string());
                self.output_lines.push("  rz           - Receive file (ZMODEM)".to_string());
                self.output_lines.push("  sz <file>    - Send file (ZMODEM)".to_string());
                self.output_lines.push("  clear        - Clear screen".to_string());
                self.output_lines.push("  help         - Show this help".to_string());
                self.output_lines.push("  quit         - Exit MistTerm".to_string());
                self.output_lines.push("".to_string());
                self.output_lines.push("Keyboard Shortcuts:".to_string());
                self.output_lines.push("  Ctrl+Q       - Quit".to_string());
                self.output_lines.push("  Ctrl+T       - Command palette".to_string());
                self.output_lines.push("  Tab          - Switch views".to_string());
            }
            Some("quit") => {
                self.should_quit = true;
            }
            Some(cmd) => {
                self.output_lines.push(format!("Unknown command: {}", cmd));
                self.output_lines.push("Type 'help' for available commands.".to_string());
            }
            None => {}
        }
    }
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();

    loop {
        terminal.draw(|f| {
            // Main layout
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),  // Title bar + tabs
                    Constraint::Min(10),     // Main content
                    Constraint::Length(3),   // Input bar
                    Constraint::Length(1),   // Status bar
                ])
                .split(f.size());

            // Title
            let title = Paragraph::new(Spans::from(vec![
                Span::styled("🌫️ MistTerm", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled("v0.1.0", Style::default().fg(Color::DarkGray)),
            ]));
            f.render_widget(title, chunks[0]);

            // Tabs
            let tabs = Tabs::new(vec![
                Spans::from("Terminal"),
                Spans::from("Files"),
                Spans::from("Settings"),
            ])
            .block(Block::default().borders(Borders::NONE))
            .select(app.tab_index)
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
            f.render_widget(tabs, chunks[0]);

            // Output area
            let output: Vec<ListItem> = app
                .output_lines
                .iter()
                .map(|line| ListItem::new(line.as_str()))
                .collect();
            let output_list = List::new(output)
                .block(Block::default().borders(Borders::ALL).title("Output"));
            f.render_widget(output_list, chunks[1]);

            // Input bar
            let input = Paragraph::new(format!("> {}", app.input_buffer))
                .block(Block::default().borders(Borders::ALL).title("Input"));
            f.render_widget(input, chunks[2]);

            // Status bar
            let status = Paragraph::new(app.status_message.as_str())
                .style(Style::default().fg(Color::White).bg(Color::DarkGray));
            f.render_widget(status, chunks[3]);
        })?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                app.handle_event(Event::Key(key));
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let _args = Args::parse();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let result = run_app(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result?;

    Ok(())
}
