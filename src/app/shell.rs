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

use crate::app::types::AppState;
use crate::debug_log;

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
        (Err(raw_err), Err(screen_err)) => Err(io::Error::new(
            raw_err.kind(),
            format!("{raw_err}; {screen_err}"),
        )),
    }
}

fn suspend_terminal_stdout() -> io::Result<()> {
    leave_tui_stdout()
}

fn resume_terminal_stdout() -> io::Result<()> {
    enter_tui_stdout()
}

struct TerminalRestoreGuard {
    already_restored: bool,
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if !self.already_restored
            && let Err(err) = resume_terminal_stdout()
        {
            debug_log!("Terminal restore failed: {err}");
        }
    }
}

/// Returns `(shell_path, flag)` for spawning a shell command.
///
/// On non-Windows, `for_menu` selects between `sh -c` (user menu) and
/// `$SHELL -c` (interactive commands). On Windows the parameter is ignored —
/// `COMSPEC` (falling back to `cmd.exe`) is always used with `/C`, because
/// menu commands are also executed through `cmd`.
#[cfg(windows)]
fn get_shell(_for_menu: bool) -> (String, &'static str) {
    let shell = std::env::var("COMSPEC")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or("cmd.exe".to_string());
    (shell, "/C")
}

#[cfg(not(windows))]
fn get_shell(for_menu: bool) -> (String, &'static str) {
    if for_menu {
        return ("sh".to_string(), "-c");
    }
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or("sh".to_string());
    (shell, "-c")
}

pub fn push_history(state: &mut AppState, cmd: &str) {
    if cmd.trim().is_empty() {
        return;
    }
    if state.command_history.back().is_none_or(|last| last != cmd) {
        state.command_history.push_back(cmd.to_string());
        if state.command_history.len() > MAX_HISTORY {
            state.command_history.pop_front();
        }
    }
}

pub fn run_shell_command(
    state: &mut AppState,
    cmd: &str,
    for_menu: bool,
    mut refresh_active: impl FnMut(&mut AppState),
) {
    if cmd.trim().is_empty() {
        return;
    }

    if suspend_terminal_stdout().is_err() {
        state.status_message = Some("Terminal suspend failed".into());
        return;
    }

    push_history(state, cmd);
    let mut restore_guard = TerminalRestoreGuard {
        already_restored: false,
    };
    let (shell, flag) = get_shell(for_menu);
    let status = Command::new(&shell)
        .arg(flag)
        .arg(cmd)
        .current_dir(state.active_panel().path())
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
        Ok(()) => restore_guard.already_restored = true,
        Err(e) => {
            state.status_message = Some(format!("Terminal restore failed: {e}"));
        }
    }
    refresh_active(state);
}

/// Toggle external panel view (Ctrl+O) - hide panels to see terminal output.
#[allow(clippy::print_stdout)]
pub fn toggle_external_view(
    state: &mut AppState,
    mut refresh_both: impl FnMut(&mut AppState),
) -> io::Result<()> {
    suspend_terminal_stdout()?;

    let mut restore_guard = TerminalRestoreGuard {
        already_restored: false,
    };

    // Show message to user.
    println!("External view active. Press Enter/Esc/Ctrl+O to return to Libre Commander.");

    // Wait for Ctrl+O or any key.
    enable_raw_mode()?;
    let wait_result = (|| -> io::Result<()> {
        loop {
            if event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS))?
                && let Event::Key(key) = event::read()?
            {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('o'), m) if m.contains(KeyModifiers::CONTROL) => break,
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => break,
                    (KeyCode::Enter, _) => break,
                    (KeyCode::Esc, _) => break,
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    let raw_result = disable_raw_mode();
    if let Err(wait_err) = wait_result {
        let reported = match raw_result {
            Err(raw_err) => io::Error::new(raw_err.kind(), format!("{raw_err}; {wait_err}")),
            Ok(()) => wait_err,
        };
        return Err(reported);
    }
    raw_result?;

    resume_terminal_stdout()?;
    restore_guard.already_restored = true;

    // Refresh display.
    refresh_both(state);

    Ok(())
}
