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

const PWD_MARKER: &[u8] = b"\0WADDLE_PWD\0";
const OUTPUT_LIMIT: usize = 128 * 1024;
const OUTPUT_TAIL: usize = 4 * 1024;
const STREAM_TRUNCATED: &[u8] = b"\n\n... output truncated while command was running ...\n\n";
const ALTERNATE_SCREEN_SEQUENCES: [&[u8]; 3] = [b"\x1b[?1049h", b"\x1b[?1047h", b"\x1b[?47h"];
const CURSOR_HIDE_SEQUENCE: &[u8] = b"\x1b[?25l";
const ERASE_DISPLAY_SEQUENCES: [&[u8]; 5] =
    [b"\x1b[J", b"\x1b[0J", b"\x1b[1J", b"\x1b[2J", b"\x1b[3J"];
const MAX_SCREEN_CONTROL_SEQUENCE_LEN: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandMode {
    Bash,
    Waddle,
}

impl CommandMode {
    fn from_prefix(prefix: char) -> Self {
        if prefix == ':' {
            Self::Waddle
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
    pub(super) successful: bool,
}

#[derive(Debug)]
pub(super) enum ShellError {
    Io(io::Error),
    Placeholder(&'static str),
    RequiresTerminal,
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Placeholder(error) => formatter.write_str(error),
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
    selected: &[PathBuf],
) -> Result<ShellReport, ShellError> {
    let mode = CommandMode::from_prefix(prefix);
    let (expanded_command, uses_selected) = expand_selected(command)?;
    if uses_selected && selected.is_empty() {
        return Err(ShellError::Placeholder(
            "$selected requires at least one selected entry",
        ));
    }
    let mut process = Command::new("bash");
    process
        .arg("-c")
        .arg(
            r#"command_text=$WADDLE_COMMAND_TEXT
unset WADDLE_COMMAND_TEXT
eval "$command_text"
status=$?
printf '\x00WADDLE_PWD\x00%s\x00' "$PWD"
exit "$status""#,
        )
        .arg("waddle")
        .env("WADDLE_COMMAND_TEXT", expanded_command)
        .current_dir(current)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if uses_selected {
        process.args(selected.iter().map(|path| selected_argument(current, path)));
    }
    let mut child = process.spawn()?;

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
    let final_directory = take_final_directory(&mut stdout).filter(|_| mode == CommandMode::Waddle);
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
        successful: status.success(),
    })
}

fn selected_argument(current: &Path, selected: &Path) -> PathBuf {
    if let Ok(relative) = selected.strip_prefix(current)
        && !relative.as_os_str().is_empty()
    {
        Path::new(".").join(relative)
    } else if selected.is_absolute() {
        selected.to_path_buf()
    } else {
        current.join(selected)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Quote {
    Unquoted,
    Single,
    Double,
}

fn expand_selected(command: &str) -> Result<(String, bool), ShellError> {
    let mut expanded = String::with_capacity(command.len());
    let mut quote = Quote::Unquoted;
    let mut index = 0;
    let mut used = false;

    while index < command.len() {
        let remaining = &command[index..];
        let character = remaining
            .chars()
            .next()
            .expect("index must remain on a character boundary");

        if character == '\\' && quote != Quote::Single {
            expanded.push(character);
            index += character.len_utf8();
            if index < command.len() {
                let escaped = command[index..]
                    .chars()
                    .next()
                    .expect("escaped character must exist");
                expanded.push(escaped);
                index += escaped.len_utf8();
            }
            continue;
        }

        match (quote, character) {
            (Quote::Unquoted, '\'') => quote = Quote::Single,
            (Quote::Unquoted, '"') => quote = Quote::Double,
            (Quote::Single, '\'') | (Quote::Double, '"') => quote = Quote::Unquoted,
            _ => {}
        }

        if character == '$'
            && quote != Quote::Single
            && let Some(length) = selected_placeholder_length(remaining)
        {
            if quote == Quote::Double {
                return Err(ShellError::Placeholder(
                    "use $selected outside quotes so paths stay separate",
                ));
            }
            expanded.push_str("\"$@\"");
            index += length;
            used = true;
            continue;
        }

        expanded.push(character);
        index += character.len_utf8();
    }

    Ok((expanded, used))
}

fn selected_placeholder_length(value: &str) -> Option<usize> {
    const PLAIN: &str = "$selected";
    const BRACED: &str = "${selected}";

    if value.starts_with(BRACED) {
        return Some(BRACED.len());
    }
    let suffix = value.strip_prefix(PLAIN)?;
    if suffix
        .chars()
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        None
    } else {
        Some(PLAIN.len())
    }
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
    let mut tail = Vec::new();
    let mut truncated = false;
    let mut chunk = [0_u8; 4096];
    let mut detector = ScreenControlDetector::default();
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let bytes = &chunk[..read];
        if output.len() < OUTPUT_LIMIT {
            let keep = (OUTPUT_LIMIT - output.len()).min(bytes.len());
            output.extend_from_slice(&bytes[..keep]);
            if keep < bytes.len() {
                truncated = true;
                append_tail(&mut tail, &bytes[keep..]);
            }
        } else {
            truncated = true;
            append_tail(&mut tail, bytes);
        }
        if detector.observe(&chunk[..read]) {
            screen_detected.store(true, Ordering::Release);
        }
    }
    if truncated {
        output.extend_from_slice(STREAM_TRUNCATED);
        output.extend_from_slice(&tail);
    }
    Ok(output)
}

fn append_tail(tail: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() >= OUTPUT_TAIL {
        tail.clear();
        tail.extend_from_slice(&bytes[bytes.len() - OUTPUT_TAIL..]);
        return;
    }
    let excess = tail
        .len()
        .saturating_add(bytes.len())
        .saturating_sub(OUTPUT_TAIL);
    if excess > 0 {
        tail.drain(..excess);
    }
    tail.extend_from_slice(bytes);
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
    fn builds_terminal_arguments_without_losing_spaces() {
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
        let report = execute(Path::new("."), '!', "true", &[]).unwrap();

        assert!(report.detail.is_empty());
    }

    #[test]
    fn selected_placeholder_passes_exact_paths_as_separate_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let selected = [
            temp.path().join("a file.txt"),
            temp.path().join("quote's.txt"),
            temp.path().join("$(touch injected)"),
        ];

        let report = execute(temp.path(), '!', "printf '<%s>\\n' $selected", &selected).unwrap();

        let expected = selected
            .iter()
            .map(|path| {
                format!(
                    "<{}>",
                    Path::new(".")
                        .join(path.file_name().expect("selected path must have a name"))
                        .display()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(report.detail, expected);
        assert!(!temp.path().join("injected").exists());
    }

    #[test]
    fn selected_placeholder_rejects_empty_selection_and_double_quotes() {
        let empty = execute(Path::new("."), '!', "printf '%s' $selected", &[]).unwrap_err();
        assert_eq!(
            empty.to_string(),
            "$selected requires at least one selected entry"
        );

        let quoted = execute(
            Path::new("."),
            '!',
            "printf '%s' \"$selected\"",
            &[PathBuf::from("one")],
        )
        .unwrap_err();
        assert_eq!(
            quoted.to_string(),
            "use $selected outside quotes so paths stay separate"
        );
    }

    #[test]
    fn selected_placeholder_respects_shell_quoting_and_variable_names() {
        assert_eq!(
            expand_selected("printf '%s' '$selected' \\$selected $selected_file").unwrap(),
            (
                "printf '%s' '$selected' \\$selected $selected_file".to_owned(),
                false
            )
        );
        assert_eq!(
            expand_selected("printf '%s\\n' ${selected} $selected").unwrap(),
            ("printf '%s\\n' \"$@\" \"$@\"".to_owned(), true)
        );
    }

    #[test]
    fn commands_without_placeholder_keep_positional_parameters_empty() {
        let report = execute(
            Path::new("."),
            '!',
            "printf '%s' \"${1-unset}\"",
            &[PathBuf::from("ignored")],
        )
        .unwrap();

        assert_eq!(report.detail, "unset");
    }

    #[test]
    fn selected_arguments_are_relative_here_and_absolute_elsewhere() {
        assert_eq!(
            selected_argument(Path::new("/work"), Path::new("/work/-option")),
            PathBuf::from("./-option")
        );
        assert_eq!(
            selected_argument(Path::new("/work"), Path::new("/elsewhere/file")),
            PathBuf::from("/elsewhere/file")
        );
        assert_eq!(
            selected_argument(Path::new("/work"), Path::new("relative/file")),
            PathBuf::from("/work/relative/file")
        );
    }

    #[test]
    fn output_is_bounded_while_the_reader_is_still_running() {
        let input = vec![b'x'; OUTPUT_LIMIT * 8];
        let output = read_output(input.as_slice(), &AtomicBool::new(false)).unwrap();

        assert!(output.len() <= OUTPUT_LIMIT + OUTPUT_TAIL + STREAM_TRUNCATED.len());
        assert!(
            output
                .windows(STREAM_TRUNCATED.len())
                .any(|part| part == STREAM_TRUNCATED)
        );
        assert_eq!(
            &output[output.len() - OUTPUT_TAIL..],
            vec![b'x'; OUTPUT_TAIL]
        );
    }
}
