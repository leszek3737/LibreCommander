use std::io;
use std::process::{Command, Stdio};
use std::time::Duration;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::Backend;

use crate::app::types::AppState;

const EVENT_POLL_TIMEOUT_MS: u64 = 100;
pub const MAX_HISTORY: usize = 100;

fn enter_tui_stdout() -> io::Result<()> {
    enable_raw_mode()?;
    if let Err(err) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide) {
        let _ = disable_raw_mode();
        return Err(err);
    }
    Ok(())
}

fn leave_tui_stdout() -> io::Result<()> {
    let raw_result = disable_raw_mode();
    let screen_result = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    );
    match (raw_result, screen_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Err(raw_err), Err(screen_err)) => {
            Err(io::Error::new(
                raw_err.kind(),
                format!("{raw_err}; {screen_err}"),
            ))
        }
    }
}

fn suspend_terminal_stdout() -> io::Result<()> {
    leave_tui_stdout()
}

fn resume_terminal_stdout() -> io::Result<()> {
    enter_tui_stdout()
}

pub fn run_shell_command(
    state: &mut AppState,
    cmd: &str,
    mut refresh_active: impl FnMut(&mut AppState),
) {
    if cmd.trim().is_empty() {
        return;
    }

    if state.command_history.back().is_none_or(|last| last != cmd) {
        state.command_history.push_back(cmd.to_string());
        if state.command_history.len() > MAX_HISTORY {
            state.command_history.pop_front();
        }
    }

    struct ShellRestoreGuard {
        restore_ok: bool,
    }

    #[allow(clippy::print_stderr)]
    impl Drop for ShellRestoreGuard {
        fn drop(&mut self) {
            if !self.restore_ok {
                if let Err(err) = resume_terminal_stdout() {
                    eprintln!("Terminal restore failed after shell command: {err}");
                }
            }
        }
    }

    let mut restore_guard = ShellRestoreGuard { restore_ok: false };
    if suspend_terminal_stdout().is_err() {
        state.status_message = Some("Terminal suspend failed".into());
        return;
    }
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&state.active_panel().path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    // Intentional stdout: TUI is suspended, user must see the prompt.
    #[allow(clippy::print_stdout)]
    match status {
        Ok(s) if s.success() => println!("\n[Command succeeded. Press Enter to return]"),
        Ok(s) => println!("\n[Command exited with status: {s}. Press Enter to return]"),
        Err(e) => println!("\n[Command failed: {e}. Press Enter to return]"),
    }
    let mut buf = String::new();
    // Intentionally ignoring read_line error: if stdin is unavailable there's nothing to wait for.
    let _ = io::stdin().read_line(&mut buf);
    match resume_terminal_stdout() {
        Ok(()) => restore_guard.restore_ok = true,
        Err(e) => {
            state.status_message = Some(format!("Terminal restore failed: {e}"));
        }
    }
    refresh_active(state);
}

/// Toggle external panel view (Ctrl+O) - hide panels to see terminal output.
#[allow(clippy::print_stdout)]
pub fn toggle_external_view<B: Backend>(
    state: &mut AppState,
    _terminal: &mut ratatui::Terminal<B>,
    mut refresh_both: impl FnMut(&mut AppState),
) -> io::Result<()> {
    suspend_terminal_stdout()?;

    struct ExternalViewRestoreGuard {
        restore_ok: bool,
    }

    impl Drop for ExternalViewRestoreGuard {
        fn drop(&mut self) {
            if !self.restore_ok {
                let _ = resume_terminal_stdout();
            }
        }
    }

    let mut restore_guard = ExternalViewRestoreGuard { restore_ok: false };

    // Show message to user.
    println!("External view active. Press Ctrl+O to return to Libre Commander.");
    println!("Press Enter to continue...");

    // Wait for Ctrl+O or any key.
    enable_raw_mode()?;
    let wait_result = (|| -> io::Result<()> {
        loop {
            if event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS))?
                && let Event::Key(key) = event::read()?
            {
                if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                // Also allow Enter to return.
                if key.code == KeyCode::Enter {
                    break;
                }
                // Esc to return.
                if key.code == KeyCode::Esc {
                    break;
                }
            }
        }
        Ok(())
    })();
    let raw_result = disable_raw_mode();
    wait_result?;
    raw_result?;

    resume_terminal_stdout()?;
    restore_guard.restore_ok = true;

    // Refresh display.
    refresh_both(state);

    Ok(())
}
