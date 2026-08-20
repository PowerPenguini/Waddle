use std::{
    ffi::OsString,
    fmt,
    io::{self, Read},
    os::unix::{ffi::OsStringExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const PWD_MARKER: &[u8] = b"\0POLAREXP_PWD\0";
const OUTPUT_LIMIT: usize = 128 * 1024;
const ALTERNATE_SCREEN_SEQUENCES: [&[u8]; 3] = [b"\x1b[?1049h", b"\x1b[?1047h", b"\x1b[?47h"];
const CURSOR_HIDE_SEQUENCE: &[u8] = b"\x1b[?25l";
const ERASE_DISPLAY_SEQUENCES: [&[u8]; 5] =
    [b"\x1b[J", b"\x1b[0J", b"\x1b[1J", b"\x1b[2J", b"\x1b[3J"];
const MAX_SCREEN_CONTROL_SEQUENCE_LEN: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CommandMode {
    Bash,
    PolarExp,
}

impl CommandMode {
    pub(super) fn from_prefix(prefix: char) -> Self {
        if prefix == ':' {
            Self::PolarExp
        } else {
            Self::Bash
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ShellReport {
    pub(super) summary: String,
    pub(super) detail: String,
    pub(super) final_directory: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) enum ShellError {
    Io(io::Error),
    RequiresTerminal,
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::RequiresTerminal => {
                formatter.write_str("command requires an interactive terminal")
            }
        }
    }
}

impl From<io::Error> for ShellError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
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

pub(super) fn is_quit(mode: CommandMode, command: &str) -> bool {
    mode == CommandMode::PolarExp && command.trim() == "q"
}

pub(super) fn is_help(mode: CommandMode, command: &str) -> bool {
    mode == CommandMode::PolarExp && matches!(command.trim(), "help" | "h")
}

pub(super) fn is_terminal(mode: CommandMode, command: &str) -> bool {
    mode == CommandMode::PolarExp && matches!(command.trim(), "terminal" | "t")
}

pub(super) fn launch_terminal(current: &Path) -> io::Result<()> {
    match spawn_terminal("xdg-terminal-exec", current) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            spawn_terminal("x-terminal-emulator", current).map_err(|fallback_error| {
                if fallback_error.kind() == io::ErrorKind::NotFound {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "no default terminal launcher found (tried xdg-terminal-exec and x-terminal-emulator)",
                    )
                } else {
                    fallback_error
                }
            })
        }
        Err(error) => Err(error),
    }
}

fn spawn_terminal(program: &str, current: &Path) -> io::Result<()> {
    let mut command = Command::new(program);
    command
        .current_dir(current)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if program == "xdg-terminal-exec" {
        command.arg(xdg_directory_argument(current));
    }
    let mut child = command.spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn xdg_directory_argument(current: &Path) -> OsString {
    let mut argument = OsString::from("--dir=");
    argument.push(current.as_os_str());
    argument
}

pub(super) fn execute(
    current: &Path,
    prefix: char,
    command: &str,
) -> Result<ShellReport, ShellError> {
    let mode = CommandMode::from_prefix(prefix);
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
    let status = wait_for_command(&mut child, &screen_detected)?;
    let mut stdout = join_output_reader(stdout_reader)?;
    let stderr = join_output_reader(stderr_reader)?;
    if screen_detected.load(Ordering::Acquire) {
        return Err(ShellError::RequiresTerminal);
    }
    let final_directory =
        take_final_directory(&mut stdout).filter(|_| mode == CommandMode::PolarExp);
    let status_text = status.code().map_or_else(
        || "terminated by signal".to_owned(),
        |code| format!("exit {code}"),
    );
    let mut detail = String::new();
    append_output(&mut detail, &stdout);
    if !stderr.is_empty() {
        if !detail.is_empty() {
            detail.push_str("\n\nstderr:\n");
        }
        append_output(&mut detail, &stderr);
    }
    truncate_output(&mut detail);
    Ok(ShellReport {
        summary: format!("{prefix}{command}  •  {status_text}"),
        detail,
        final_directory,
    })
}

fn wait_for_command(
    child: &mut Child,
    screen_detected: &AtomicBool,
) -> Result<ExitStatus, ShellError> {
    loop {
        if screen_detected.load(Ordering::Acquire) {
            return terminate_process_group(child).map_err(Into::into);
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(10));
    }
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
) -> Result<Vec<u8>, ShellError> {
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
    use super::*;

    #[test]
    fn separates_bash_and_internal_commands() {
        assert!(!is_quit(CommandMode::Bash, "q"));
        assert!(is_quit(CommandMode::PolarExp, " q "));
        assert!(!is_quit(CommandMode::PolarExp, "q!"));
        assert!(!is_help(CommandMode::Bash, "help"));
        assert!(is_help(CommandMode::PolarExp, " help "));
        assert!(is_help(CommandMode::PolarExp, "h"));
        assert!(!is_terminal(CommandMode::Bash, "terminal"));
        assert!(is_terminal(CommandMode::PolarExp, " terminal "));
        assert!(is_terminal(CommandMode::PolarExp, "t"));
        assert!(!is_terminal(CommandMode::PolarExp, "term"));
        assert_eq!(
            xdg_directory_argument(Path::new("/tmp/folder with spaces")),
            OsString::from("--dir=/tmp/folder with spaces")
        );
    }

    #[test]
    fn recognizes_terminal_screen_control_sequences() {
        let mut detector = ScreenControlDetector::default();
        assert!(!detector.observe(b"\x1b[?25"));
        assert!(!detector.observe(b"lhello\x1b[?25h"));
        assert!(detector.observe(b"\x1b[?1049h"));
    }

    #[test]
    fn successful_silent_command_has_empty_detail() {
        let report = execute(Path::new("."), '!', "true").unwrap();

        assert!(report.detail.is_empty());
    }
}
