use std::{
    ffi::OsString,
    fmt,
    io::{self, Read},
    os::unix::ffi::OsStringExt,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use super::Explorer;
use crate::app::{
    state::{NavigationKind, PendingNavigation},
    view::show_error_window,
};

const PWD_MARKER: &[u8] = b"\0POLAREXP_PWD\0";
const OUTPUT_LIMIT: usize = 128 * 1024;
const ALTERNATE_SCREEN_SEQUENCES: [&[u8]; 3] = [b"\x1b[?1049h", b"\x1b[?1047h", b"\x1b[?47h"];
const CURSOR_HIDE_SEQUENCE: &[u8] = b"\x1b[?25l";
const ERASE_DISPLAY_SEQUENCES: [&[u8]; 5] =
    [b"\x1b[J", b"\x1b[0J", b"\x1b[1J", b"\x1b[2J", b"\x1b[3J"];
const MAX_SCREEN_CONTROL_SEQUENCE_LEN: usize = 8;

#[derive(Debug)]
struct ShellExecution {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    final_directory: Option<PathBuf>,
}

#[derive(Debug)]
enum ShellCommandError {
    Io(io::Error),
    RequiresTerminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandMode {
    Bash,
    PolarExp,
}

impl CommandMode {
    fn from_prefix(prefix: &str) -> Self {
        if prefix == ":" {
            Self::PolarExp
        } else {
            Self::Bash
        }
    }

    fn applies_directory_change(self) -> bool {
        self == Self::PolarExp
    }
}

#[derive(Default)]
struct ScreenControlDetector {
    cursor_hidden: bool,
    tail: Vec<u8>,
}

impl ScreenControlDetector {
    fn observe(&mut self, bytes: &[u8]) -> bool {
        let mut scan = std::mem::take(&mut self.tail);
        scan.extend_from_slice(bytes);

        if contains_any_sequence(&scan, &ALTERNATE_SCREEN_SEQUENCES) {
            return true;
        }
        self.cursor_hidden |= contains_sequence(&scan, CURSOR_HIDE_SEQUENCE);
        if self.cursor_hidden && contains_any_sequence(&scan, &ERASE_DISPLAY_SEQUENCES) {
            return true;
        }

        let tail_start = scan
            .len()
            .saturating_sub(MAX_SCREEN_CONTROL_SEQUENCE_LEN - 1);
        self.tail.extend_from_slice(&scan[tail_start..]);
        false
    }
}

impl fmt::Display for ShellCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::RequiresTerminal => {
                formatter.write_str("command requires an interactive terminal")
            }
        }
    }
}

impl From<io::Error> for ShellCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl Explorer {
    pub(super) fn run_shell_command(&self, command: &str) {
        let prefix = self
            .ui
            .upgrade()
            .map(|window| window.get_command_prefix().to_string())
            .unwrap_or_else(|| "!".to_owned());
        let mode = CommandMode::from_prefix(&prefix);
        if is_quit_command(mode, command) {
            let _ = slint::quit_event_loop();
            return;
        }
        if command.trim().is_empty()
            || !self
                .ui
                .upgrade()
                .is_some_and(|ui| !ui.get_busy() && !ui.get_navigation_loading())
        {
            return;
        }

        let command = command.to_owned();
        let current = self.state.lock().unwrap().current.clone();
        let tasks = self.operation_tasks.clone();
        let navigation = self.navigation_context();
        let ui = self.ui.clone();
        if let Some(window) = ui.upgrade() {
            window.set_command_output_active(false);
            window.set_busy(true);
            window.set_status_text(format!("Running {prefix}{command}…").into());
        }

        let fallback_ui = ui.clone();
        if !tasks.execute(move || {
            let result = execute_shell_command(&current, &command);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                window.set_busy(false);
                match result {
                    Ok(result) => {
                        let final_directory = result.final_directory.clone();
                        let show_output = !result.status.success()
                            || !result.stdout.is_empty()
                            || !result.stderr.is_empty();
                        if mode.applies_directory_change()
                            && final_directory
                                .as_ref()
                                .is_some_and(|path| path != &current)
                        {
                            navigation.request(PendingNavigation {
                                requested: final_directory.unwrap(),
                                kind: NavigationKind::Forward { remember: true },
                                select: None,
                            });
                        } else {
                            navigation.refresh(None, false);
                        }
                        if show_output {
                            show_shell_output(&window, &prefix, &command, &result);
                        }
                    }
                    Err(ShellCommandError::RequiresTerminal) => {
                        show_terminal_required(&window, &prefix, &command);
                    }
                    Err(ShellCommandError::Io(error)) => {
                        show_error_window(&window, format!("Could not run Bash: {error}"));
                    }
                }
            });
        }) && let Some(window) = fallback_ui.upgrade()
        {
            window.set_busy(false);
            show_error_window(&window, "Could not queue the Bash command".to_owned());
        }
    }
}

fn is_quit_command(mode: CommandMode, command: &str) -> bool {
    mode == CommandMode::PolarExp && command.trim() == "q"
}

fn execute_shell_command(
    current: &Path,
    command: &str,
) -> Result<ShellExecution, ShellCommandError> {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(
            r#"command_text=$POLAREXP_COMMAND_TEXT
unset POLAREXP_COMMAND_TEXT
eval "$command_text"
status=$?
printf '\x00POLAREXP_PWD\x00%s\x00' "$PWD"
exit "$status""#,
        )
        .arg("polarexp")
        .env("POLAREXP_COMMAND_TEXT", command)
        .current_dir(current)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let screen_detected = Arc::new(AtomicBool::new(false));
    let stdout_reader = spawn_output_reader(
        child.stdout.take().expect("piped stdout must be available"),
        Arc::clone(&screen_detected),
    );
    let stderr_reader = spawn_output_reader(
        child.stderr.take().expect("piped stderr must be available"),
        Arc::clone(&screen_detected),
    );

    let status = loop {
        if screen_detected.load(Ordering::Acquire) {
            let status = terminate_process_group(&mut child)?;
            break status;
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(10));
    };

    let mut stdout = join_output_reader(stdout_reader)?;
    let stderr = join_output_reader(stderr_reader)?;
    if screen_detected.load(Ordering::Acquire) {
        return Err(ShellCommandError::RequiresTerminal);
    }
    let final_directory = take_final_directory(&mut stdout);
    Ok(ShellExecution {
        status,
        stdout,
        stderr,
        final_directory,
    })
}

fn spawn_output_reader<R>(
    reader: R,
    screen_detected: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || read_output(reader, &screen_detected))
}

fn read_output<R: Read>(mut reader: R, screen_detected: &AtomicBool) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut detector = ScreenControlDetector::default();
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(output);
        }
        output.extend_from_slice(&chunk[..read]);
        if detector.observe(&chunk[..read]) {
            screen_detected.store(true, Ordering::Release);
        }
    }
}

fn contains_any_sequence(bytes: &[u8], sequences: &[&[u8]]) -> bool {
    sequences.iter().any(|sequence| {
        bytes
            .windows(sequence.len())
            .any(|window| window == *sequence)
    })
}

fn contains_sequence(bytes: &[u8], sequence: &[u8]) -> bool {
    bytes
        .windows(sequence.len())
        .any(|window| window == sequence)
}

fn join_output_reader(
    reader: thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, ShellCommandError> {
    reader
        .join()
        .map_err(|_| io::Error::other("Bash output reader panicked"))?
        .map_err(Into::into)
}

fn terminate_process_group(child: &mut Child) -> io::Result<ExitStatus> {
    signal_process_group(child.id(), libc::SIGTERM);
    for _ in 0..20 {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(10));
    }
    signal_process_group(child.id(), libc::SIGKILL);
    child.wait()
}

fn signal_process_group(pid: u32, signal: libc::c_int) {
    // The child starts its own process group, so a negative PID targets only
    // this command and all of the processes it launched.
    unsafe {
        libc::kill(-(pid as libc::pid_t), signal);
    }
}

fn take_final_directory(stdout: &mut Vec<u8>) -> Option<PathBuf> {
    let marker = stdout
        .windows(PWD_MARKER.len())
        .rposition(|window| window == PWD_MARKER)?;
    let path_start = marker + PWD_MARKER.len();
    let path_end = path_start
        + stdout
            .get(path_start..)?
            .iter()
            .position(|byte| *byte == 0)?;
    let path = PathBuf::from(OsString::from_vec(stdout[path_start..path_end].to_vec()));
    stdout.truncate(marker);
    Some(path)
}

fn show_shell_output(
    window: &crate::AppWindow,
    prefix: &str,
    command: &str,
    execution: &ShellExecution,
) {
    let status = execution.status.code().map_or_else(
        || "terminated by signal".to_owned(),
        |code| format!("exit {code}"),
    );
    let mut detail = String::new();
    append_output(&mut detail, &execution.stdout);
    if !execution.stderr.is_empty() {
        if !detail.is_empty() {
            detail.push_str("\n\nstderr:\n");
        }
        append_output(&mut detail, &execution.stderr);
    }
    if detail.is_empty() {
        detail.push_str("The command produced no output.");
    }
    truncate_output(&mut detail);

    window.set_command_output_summary(format!("{prefix}{command}  •  {status}").into());
    window.set_command_output_detail(detail.into());
    window.set_command_output_active(true);
}

fn show_terminal_required(window: &crate::AppWindow, prefix: &str, command: &str) {
    window.set_command_output_summary(
        format!("{prefix}{command}  •  interactive terminal required").into(),
    );
    window.set_command_output_detail(
        "This command tried to take over the terminal screen, so PolarExp stopped it.".into(),
    );
    window.set_command_output_active(true);
}

fn append_output(target: &mut String, bytes: &[u8]) {
    target.push_str(String::from_utf8_lossy(bytes).trim_end());
}

fn truncate_output(output: &mut String) {
    if output.len() <= OUTPUT_LIMIT {
        return;
    }
    let mut boundary = OUTPUT_LIMIT;
    while !output.is_char_boundary(boundary) {
        boundary -= 1;
    }
    output.truncate(boundary);
    output.push_str("\n\n… output truncated at 128 KiB");
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        CommandMode, ScreenControlDetector, ShellCommandError, execute_shell_command,
        is_quit_command, take_final_directory,
    };

    #[test]
    fn separates_bash_and_polarexp_command_side_effects() {
        let bash = CommandMode::from_prefix("!");
        let polarexp = CommandMode::from_prefix(":");

        assert!(!bash.applies_directory_change());
        assert!(polarexp.applies_directory_change());
        assert!(is_quit_command(polarexp, "q"));
        assert!(is_quit_command(polarexp, "  q  "));
        assert!(!is_quit_command(bash, "q"));
        assert!(!is_quit_command(polarexp, "quit"));
        assert!(!is_quit_command(polarexp, "q!"));
    }

    #[test]
    fn captures_stdout_status_and_final_directory() {
        let temp = tempfile::tempdir().unwrap();
        let child = temp.path().join("child");
        std::fs::create_dir(&child).unwrap();

        let result = execute_shell_command(temp.path(), "printf '<%s>' \"$1\"; cd child").unwrap();

        assert!(result.status.success());
        assert_eq!(result.stdout, b"<>");
        assert!(result.stderr.is_empty());
        assert_eq!(result.final_directory.as_deref(), Some(child.as_path()));
    }

    #[test]
    fn preserves_failure_status_and_stderr() {
        let temp = tempfile::tempdir().unwrap();

        let result = execute_shell_command(temp.path(), "printf problem >&2; false").unwrap();

        assert_eq!(result.status.code(), Some(1));
        assert_eq!(result.stderr, b"problem");
        assert_eq!(result.final_directory.as_deref(), Some(temp.path()));
    }

    #[test]
    fn missing_marker_leaves_stdout_untouched() {
        let mut output = b"ordinary output".to_vec();

        assert!(take_final_directory(&mut output).is_none());
        assert_eq!(output, b"ordinary output");
    }

    #[test]
    fn recognizes_terminal_screen_control_sequences() {
        let mut detector = ScreenControlDetector::default();
        assert!(detector.observe(b"before\x1b[?1049hafter"));

        let mut detector = ScreenControlDetector::default();
        assert!(!detector.observe(b"before\x1b[?25l"));
        assert!(detector.observe(b"\x1b[Jafter"));

        let mut detector = ScreenControlDetector::default();
        assert!(!detector.observe(b"\x1b[31mred text\x1b[0m"));
    }

    #[test]
    fn stops_a_command_that_takes_over_the_terminal_screen() {
        let temp = tempfile::tempdir().unwrap();
        let started = Instant::now();

        let result = execute_shell_command(temp.path(), "printf '\\033[?25l\\033[J'; sleep 5");

        assert!(matches!(result, Err(ShellCommandError::RequiresTerminal)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
