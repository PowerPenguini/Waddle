use std::path::{Path, PathBuf};

use super::browser_input;
use super::shell::{self, ShellError, ShellReport};

const COMMAND_HELP: &str = "\
Commands
  :help, :h  Show this reference
  :terminal, :t  Open a terminal here
  :refresh  Refresh the current view
  :diagnostics, :diag  Show local command failures
  :set [all|OPTION=VALUE ...]  Change session settings
  :setlocal [all|OPTION=VALUE ...]
    Change folder settings for this session
  :favorite [list]  List Favorites
  :favorite add [LABEL]  Add the current folder
  :favorite remove INDEX  Remove a Favorite
  :recent [open|clear|enable|disable]  Manage Recent
  :volume mount|unmount|eject NAME  Manage a volume
  :properties, :props [PATH]  Inspect an entry
  :chmod MODE [PATH ...]  Change permissions
  :open-with [APP_ID] [-- PATH]  Choose or open an application
  :default-app APP_ID [-- PATH]  Set the default application
  :cd PATH  Change Waddle's current directory
  :q  Quit Waddle
  :COMMAND  Run Bash and keep its final directory
  !COMMAND  Run Bash without changing Waddle's directory
  $selected  Selected paths in Bash; use outside quotes

  Targets default to selection; relative paths use this folder
  Quote paths with spaces; put -- before Open With paths

Command prompt
  Tab         Complete a command name
  Enter       Submit the command
  Esc         Cancel input or close command output

Settings
  Persistent settings: $XDG_CONFIG_HOME/waddle/waddlerc
  view=grid|list
  sort=name|type|size|modified
  direction=ascending|descending
  hidden=true|false
  file-click=single|double
  folder-click=single|double
  icons=waddle|system
  high-contrast=auto|true|false
  reduced-motion=auto|true|false
  reduced-transparency=auto|true|false
  tree=true|false
  startup=last|cwd (waddlerc only)";

pub(super) trait Adapter {
    fn execute(
        &self,
        current: &Path,
        prefix: char,
        command: &str,
        selected: &[PathBuf],
    ) -> Result<ShellReport, Failure>;

    fn launch_terminal(&self, current: &Path) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ProcessAdapter;

impl Adapter for ProcessAdapter {
    fn execute(
        &self,
        current: &Path,
        prefix: char,
        command: &str,
        selected: &[PathBuf],
    ) -> Result<ShellReport, Failure> {
        shell::execute(current, prefix, command, selected).map_err(|error| match error {
            ShellError::RequiresTerminal => Failure::RequiresTerminal,
            ShellError::Io(error) => Failure::Other(error.to_string()),
            ShellError::Placeholder(error) => Failure::Other(error.to_owned()),
        })
    }

    fn launch_terminal(&self, current: &Path) -> Result<(), String> {
        shell::launch_terminal(current).map_err(|error| error.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Output {
    pub(super) summary: String,
    pub(super) detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Failure {
    RequiresTerminal,
    Other(String),
}

#[derive(Clone, Debug)]
enum ExecutionKind {
    Shell { prefix: char, command: String },
    Terminal,
}

#[derive(Clone, Debug)]
pub(super) struct Execution {
    current: PathBuf,
    selected: Vec<PathBuf>,
    kind: ExecutionKind,
}

impl Execution {
    pub(super) fn status(&self) -> String {
        match &self.kind {
            ExecutionKind::Shell { prefix, command } => format!("Running {prefix}{command}…"),
            ExecutionKind::Terminal => {
                format!("Opening terminal in {}…", self.current.display())
            }
        }
    }

    pub(super) fn with_selected(mut self, selected: Vec<PathBuf>) -> Self {
        self.selected = selected;
        self
    }

    pub(super) fn run<A: Adapter>(self, adapter: &A) -> Completion {
        match self.kind {
            ExecutionKind::Shell { prefix, command } => {
                Completion::Shell(adapter.execute(&self.current, prefix, &command, &self.selected))
            }
            ExecutionKind::Terminal => Completion::Terminal {
                directory: self.current.clone(),
                result: adapter.launch_terminal(&self.current),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Completion {
    Shell(Result<ShellReport, Failure>),
    Terminal {
        directory: PathBuf,
        result: Result<(), String>,
    },
}

#[derive(Debug)]
pub(super) enum CommandAction {
    None,
    Error(String),
    Quit,
    OutputChanged,
    Refresh,
    Diagnostics,
    ChangeSettings {
        local: bool,
        arguments: String,
    },
    ManageFavorite(String),
    ManageRecent(String),
    ManageVolume(String),
    ShowProperties {
        target: Option<PathBuf>,
    },
    ChangePermissions {
        mode: String,
        targets: Vec<PathBuf>,
    },
    OpenWith {
        application: String,
        default: bool,
        target: Option<PathBuf>,
    },
    Execute(Execution),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Consequences {
    pub(super) status: Option<String>,
    pub(super) error: Option<String>,
    pub(super) refresh: bool,
    pub(super) navigate: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CommandSession {
    prefix: Option<char>,
    text: String,
    output: Option<Output>,
}

impl CommandSession {
    pub(super) fn begin(&mut self, prefix: char) {
        self.prefix = Some(prefix);
        self.text.clear();
        self.output = None;
    }

    pub(super) fn cancel(&mut self) {
        self.prefix = None;
        self.text.clear();
    }

    pub(super) fn change(&mut self, value: String) {
        self.text = value;
    }

    pub(super) fn prefix(&self) -> Option<char> {
        self.prefix
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn output(&self) -> Option<&Output> {
        self.output.as_ref()
    }

    pub(super) fn close_output(&mut self) {
        self.output = None;
    }

    pub(super) fn submit(&mut self, current: PathBuf) -> CommandAction {
        let Some(prefix) = self.prefix.take() else {
            return CommandAction::None;
        };
        let command = std::mem::take(&mut self.text);
        let trimmed = command.trim();
        if prefix == ':' && trimmed == "q" {
            return CommandAction::Quit;
        }
        if prefix == ':' && matches!(trimmed, "help" | "h") {
            self.output = Some(Output {
                summary: ":help  •  Waddle commands".to_owned(),
                detail: format!("{COMMAND_HELP}\n\n{}", browser_input::HELP),
            });
            return CommandAction::OutputChanged;
        }
        if prefix == ':' && matches!(trimmed, "terminal" | "t") {
            return CommandAction::Execute(Execution {
                current,
                selected: Vec::new(),
                kind: ExecutionKind::Terminal,
            });
        }
        if prefix == ':' && trimmed == "refresh" {
            return CommandAction::Refresh;
        }
        if prefix == ':' && matches!(trimmed, "diagnostics" | "diag") {
            return CommandAction::Diagnostics;
        }
        if prefix == ':'
            && let Some((command, arguments)) = trimmed
                .split_once(char::is_whitespace)
                .or(Some((trimmed, "")))
            && matches!(command, "set" | "setlocal")
        {
            return CommandAction::ChangeSettings {
                local: command == "setlocal",
                arguments: arguments.trim().to_owned(),
            };
        }
        if prefix == ':' && (trimmed == "favorite" || trimmed.starts_with("favorite ")) {
            return CommandAction::ManageFavorite(
                trimmed
                    .strip_prefix("favorite")
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            );
        }
        if prefix == ':' && (trimmed == "recent" || trimmed.starts_with("recent ")) {
            return CommandAction::ManageRecent(
                trimmed
                    .strip_prefix("recent")
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            );
        }
        if prefix == ':' && (trimmed == "volume" || trimmed.starts_with("volume ")) {
            return CommandAction::ManageVolume(
                trimmed
                    .strip_prefix("volume")
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            );
        }
        if prefix == ':'
            && let Some((command, arguments)) = command_and_arguments(trimmed)
            && matches!(command, "properties" | "props")
        {
            return match parse_single_target(&current, arguments, ":properties [PATH]") {
                Ok(target) => CommandAction::ShowProperties { target },
                Err(error) => CommandAction::Error(error),
            };
        }
        if prefix == ':' && (trimmed == "chmod" || trimmed.starts_with("chmod ")) {
            let arguments = trimmed.strip_prefix("chmod").unwrap_or_default().trim();
            return match parse_chmod_arguments(&current, arguments) {
                Ok((mode, targets)) => CommandAction::ChangePermissions { mode, targets },
                Err(error) => CommandAction::Error(error),
            };
        }
        if prefix == ':'
            && let Some((command, application)) = trimmed
                .split_once(char::is_whitespace)
                .or(Some((trimmed, "")))
            && matches!(command, "open-with" | "default-app")
        {
            return match parse_open_with_arguments(&current, command, application.trim()) {
                Ok((application, target)) => CommandAction::OpenWith {
                    application,
                    default: command == "default-app",
                    target,
                },
                Err(error) => CommandAction::Error(error),
            };
        }
        if trimmed.is_empty() {
            return CommandAction::None;
        }
        CommandAction::Execute(Execution {
            current,
            selected: Vec::new(),
            kind: ExecutionKind::Shell { prefix, command },
        })
    }

    pub(super) fn complete(
        &mut self,
        completion: Result<Completion, String>,
        current: &Path,
    ) -> Consequences {
        match completion {
            Ok(Completion::Terminal { directory, result }) => match result {
                Ok(()) => {
                    self.output = None;
                    Consequences {
                        status: Some(format!("Opened terminal in {}", directory.display())),
                        ..Consequences::default()
                    }
                }
                Err(error) => {
                    self.output = Some(Output {
                        summary: ":terminal  •  error".to_owned(),
                        detail: format!("Could not open the default terminal: {error}"),
                    });
                    Consequences::default()
                }
            },
            Ok(Completion::Shell(Ok(report))) => {
                let ShellReport {
                    summary,
                    detail,
                    final_directory,
                    successful: _,
                } = report;
                let status = if detail.trim().is_empty() {
                    self.output = None;
                    Some(summary)
                } else {
                    self.output = Some(Output { summary, detail });
                    None
                };
                Consequences {
                    status,
                    refresh: true,
                    navigate: final_directory.filter(|path| path != current),
                    error: None,
                }
            }
            Ok(Completion::Shell(Err(Failure::RequiresTerminal))) => {
                self.output = Some(Output {
                    summary: "interactive terminal required".to_owned(),
                    detail:
                        "This command tried to take over the terminal screen, so Waddle stopped it."
                            .to_owned(),
                });
                Consequences::default()
            }
            Ok(Completion::Shell(Err(Failure::Other(error)))) => Consequences {
                error: Some(format!("Could not run Bash: {error}")),
                ..Consequences::default()
            },
            Err(error) => Consequences {
                error: Some(error),
                ..Consequences::default()
            },
        }
    }

    pub(super) fn show_diagnostics(&mut self, detail: String) {
        self.output = Some(Output {
            summary: ":diagnostics  •  local command failures".to_owned(),
            detail,
        });
    }

    pub(super) fn show_settings(&mut self, detail: String) {
        self.output = Some(Output {
            summary: ":set  •  session settings".to_owned(),
            detail,
        });
    }

    pub(super) fn show_output(&mut self, summary: String, detail: String) {
        self.output = Some(Output { summary, detail });
    }

    pub(super) fn complete_setting(&mut self) -> bool {
        const CANDIDATES: &[&str] = &[
            "set all",
            "set view=",
            "set sort=",
            "set direction=",
            "set hidden=",
            "set file-click=",
            "set folder-click=",
            "set icons=",
            "set high-contrast=",
            "set reduced-motion=",
            "set reduced-transparency=",
            "set tree=",
            "set view=grid",
            "set view=list",
            "set sort=name",
            "set sort=modified",
            "set sort=size",
            "set sort=type",
            "set direction=ascending",
            "set direction=descending",
            "set hidden=true",
            "set hidden=false",
            "set file-click=single",
            "set file-click=double",
            "set folder-click=single",
            "set folder-click=double",
            "set icons=waddle",
            "set icons=system",
            "set high-contrast=auto",
            "set high-contrast=true",
            "set high-contrast=false",
            "set reduced-motion=auto",
            "set reduced-motion=true",
            "set reduced-motion=false",
            "set reduced-transparency=auto",
            "set reduced-transparency=true",
            "set reduced-transparency=false",
            "set tree=true",
            "set tree=false",
            "setlocal all",
            "setlocal view=",
            "setlocal sort=",
            "setlocal direction=",
            "setlocal hidden=",
            "setlocal view&",
            "setlocal sort&",
            "setlocal direction&",
            "setlocal hidden&",
            "setlocal view=grid",
            "setlocal view=list",
            "setlocal sort=name",
            "setlocal sort=modified",
            "setlocal sort=size",
            "setlocal sort=type",
            "setlocal direction=ascending",
            "setlocal direction=descending",
            "setlocal hidden=true",
            "setlocal hidden=false",
        ];
        let matches = CANDIDATES
            .iter()
            .filter(|candidate| candidate.starts_with(&self.text))
            .copied()
            .collect::<Vec<_>>();
        let Some(first) = matches.first() else {
            return false;
        };
        let common_length = matches
            .iter()
            .skip(1)
            .fold(first.len(), |length, candidate| {
                first
                    .bytes()
                    .zip(candidate.bytes())
                    .take_while(|(left, right)| left == right)
                    .count()
                    .min(length)
            });
        if common_length <= self.text.len() {
            return false;
        }
        self.text = first[..common_length].to_owned();
        true
    }
}

fn command_and_arguments(command: &str) -> Option<(&str, &str)> {
    command
        .split_once(char::is_whitespace)
        .or(Some((command, "")))
        .map(|(command, arguments)| (command, arguments.trim()))
}

fn parse_single_target(
    current: &Path,
    arguments: &str,
    usage: &str,
) -> Result<Option<PathBuf>, String> {
    let targets = parse_targets(current, arguments)?;
    match targets.as_slice() {
        [] => Ok(None),
        [target] => Ok(Some(target.clone())),
        _ => Err(format!("Usage: {usage}")),
    }
}

fn parse_chmod_arguments(
    current: &Path,
    arguments: &str,
) -> Result<(String, Vec<PathBuf>), String> {
    let words = shlex::split(arguments)
        .ok_or_else(|| "Could not parse :chmod arguments: unmatched quote".to_owned())?;
    let Some((mode, targets)) = words.split_first() else {
        return Ok((String::new(), Vec::new()));
    };
    Ok((
        mode.clone(),
        targets
            .iter()
            .map(|target| resolve_target(current, target))
            .collect(),
    ))
}

fn parse_open_with_arguments(
    current: &Path,
    command: &str,
    arguments: &str,
) -> Result<(String, Option<PathBuf>), String> {
    let Some(words) = shlex::split(arguments) else {
        return Err("Could not parse Open With arguments: unmatched quote".to_owned());
    };
    let Some(separator) = words.iter().position(|word| word == "--") else {
        let application = match words.as_slice() {
            [application] => application.clone(),
            _ => arguments.to_owned(),
        };
        return Ok((application, None));
    };
    let application = words[..separator].join(" ");
    let targets = &words[separator + 1..];
    let [target] = targets else {
        let application = if command == "open-with" {
            "[APP_ID]"
        } else {
            "APP_ID"
        };
        return Err(format!("Usage: :{command} {application} [-- PATH]"));
    };
    if command == "default-app" && application.is_empty() {
        return Err("Usage: :default-app APP_ID [-- PATH]".to_owned());
    }
    Ok((application, Some(resolve_target(current, target))))
}

fn parse_targets(current: &Path, arguments: &str) -> Result<Vec<PathBuf>, String> {
    shlex::split(arguments)
        .ok_or_else(|| "Could not parse path: unmatched quote".to_owned())
        .map(|targets| {
            targets
                .into_iter()
                .map(|target| resolve_target(current, &target))
                .collect()
        })
}

fn resolve_target(current: &Path, target: &str) -> PathBuf {
    let target = PathBuf::from(target);
    if target.is_absolute() {
        target
    } else {
        current.join(target)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct MemoryAdapter {
        calls: Mutex<Vec<String>>,
        selected_calls: Mutex<Vec<Vec<PathBuf>>>,
        shell_result: Mutex<Option<Result<ShellReport, Failure>>>,
        terminal_result: Mutex<Option<Result<(), String>>>,
    }

    impl Adapter for MemoryAdapter {
        fn execute(
            &self,
            _: &Path,
            prefix: char,
            command: &str,
            selected: &[PathBuf],
        ) -> Result<ShellReport, Failure> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{prefix}{command}"));
            self.selected_calls.lock().unwrap().push(selected.to_vec());
            self.shell_result.lock().unwrap().take().unwrap()
        }

        fn launch_terminal(&self, current: &Path) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("terminal:{}", current.display()));
            self.terminal_result.lock().unwrap().take().unwrap()
        }
    }

    #[test]
    fn help_is_interpreted_inside_the_session() {
        let mut session = CommandSession::default();
        session.begin(':');
        session.change("help".to_owned());

        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::OutputChanged
        ));
        let output = session.output().unwrap();
        assert!(output.summary.starts_with(":help"));
        assert!(output.detail.contains(":terminal, :t"));
        assert!(output.detail.contains(":favorite add [LABEL]"));
        assert!(output.detail.contains("$selected"));
        assert!(output.detail.contains("Tab / Shift+Tab"));
        assert!(output.detail.contains("\"_dd / \"_d{motion}"));
        assert!(output.detail.contains("Name/Type/Size/Modified header"));
        assert!(
            output.detail.lines().all(|line| line.chars().count() <= 64),
            "help lines must fit the narrow output panel"
        );
        assert_eq!(session.prefix(), None);
        assert!(session.text().is_empty());
    }

    #[test]
    fn refresh_is_a_builtin_command_not_a_shell_process() {
        let mut session = CommandSession::default();
        session.begin(':');
        session.change("refresh".to_owned());

        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::Refresh
        ));
    }

    #[test]
    fn diagnostics_is_a_builtin_command_not_a_shell_process() {
        let mut session = CommandSession::default();
        session.begin(':');
        session.change("diagnostics".to_owned());

        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::Diagnostics
        ));
    }

    #[test]
    fn metadata_commands_keep_selection_as_the_default_target() {
        let mut session = CommandSession::default();
        session.begin(':');
        session.change("chmod 640".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ChangePermissions { mode, targets }
                if mode == "640" && targets.is_empty()
        ));

        session.begin(':');
        session.change("default-app org.example.Editor.desktop".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::OpenWith {
                application,
                default: true,
                target: None,
            } if application == "org.example.Editor.desktop"
        ));
    }

    #[test]
    fn metadata_commands_resolve_names_and_paths_from_the_current_folder() {
        let mut session = CommandSession::default();

        session.begin(':');
        session.change("properties \"notes from today.txt\"".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ShowProperties { target: Some(target) }
                if target.as_path() == Path::new("/work/notes from today.txt")
        ));

        session.begin(':');
        session.change("chmod 640 notes.txt \"archive copy.txt\" /tmp/shared.txt".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ChangePermissions { mode, targets }
                if mode == "640"
                    && targets == [
                        PathBuf::from("/work/notes.txt"),
                        PathBuf::from("/work/archive copy.txt"),
                        PathBuf::from("/tmp/shared.txt"),
                    ]
        ));

        session.begin(':');
        session
            .change("open-with org.example.Editor.desktop -- \"notes from today.txt\"".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::OpenWith {
                application,
                default: false,
                target: Some(target),
            } if application == "org.example.Editor.desktop"
                && target.as_path() == Path::new("/work/notes from today.txt")
        ));

        session.begin(':');
        session.change("open-with -- notes.txt".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::OpenWith {
                application,
                default: false,
                target: Some(target),
            } if application.is_empty() && target.as_path() == Path::new("/work/notes.txt")
        ));
    }

    #[test]
    fn metadata_commands_reject_ambiguous_or_unclosed_paths() {
        let mut session = CommandSession::default();

        session.begin(':');
        session.change("properties one two".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::Error(error) if error == "Usage: :properties [PATH]"
        ));

        session.begin(':');
        session.change("default-app Editor -- one two".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::Error(error) if error == "Usage: :default-app APP_ID [-- PATH]"
        ));
    }

    #[test]
    fn management_commands_return_semantic_actions() {
        let mut session = CommandSession::default();

        session.begin(':');
        session.change("favorite add Work".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ManageFavorite(arguments) if arguments == "add Work"
        ));

        session.begin(':');
        session.change("recent clear".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ManageRecent(arguments) if arguments == "clear"
        ));

        session.begin(':');
        session.change("volume mount Archive".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ManageVolume(arguments) if arguments == "mount Archive"
        ));

        session.begin(':');
        session.change("properties".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ShowProperties { target: None }
        ));
    }

    #[test]
    fn setting_commands_preserve_scope_and_complete_unique_names() {
        let mut session = CommandSession::default();
        session.begin(':');
        session.change("setlocal view=list".to_owned());
        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            CommandAction::ChangeSettings {
                local: true,
                arguments
            } if arguments == "view=list"
        ));

        session.begin(':');
        session.change("set vie".to_owned());
        assert!(session.complete_setting());
        assert_eq!(session.text(), "set view=");
        session.change("set view=l".to_owned());
        assert!(session.complete_setting());
        assert_eq!(session.text(), "set view=list");
        session.change("set file-c".to_owned());
        assert!(session.complete_setting());
        assert_eq!(session.text(), "set file-click=");
        session.change("set folder-click=d".to_owned());
        assert!(session.complete_setting());
        assert_eq!(session.text(), "set folder-click=double");
        session.change("set icons=s".to_owned());
        assert!(session.complete_setting());
        assert_eq!(session.text(), "set icons=system");
        session.change("set folders".to_owned());
        assert!(!session.complete_setting());
    }

    #[test]
    fn memory_adapter_is_the_second_adapter_at_the_command_seam() {
        let adapter = MemoryAdapter {
            shell_result: Mutex::new(Some(Ok(ShellReport {
                summary: "!true  •  exit 0".to_owned(),
                detail: String::new(),
                final_directory: None,
                successful: true,
            }))),
            ..MemoryAdapter::default()
        };
        let mut session = CommandSession::default();
        session.begin('!');
        session.change("true".to_owned());
        let CommandAction::Execute(execution) = session.submit(PathBuf::from("/work")) else {
            panic!("expected execution");
        };
        let selected = vec![PathBuf::from("/work/a.txt"), PathBuf::from("/work/b c.txt")];

        let completion = execution.with_selected(selected.clone()).run(&adapter);
        let consequences = session.complete(Ok(completion), Path::new("/work"));

        assert_eq!(adapter.calls.lock().unwrap().as_slice(), ["!true"]);
        assert_eq!(
            adapter.selected_calls.lock().unwrap().as_slice(),
            [selected]
        );
        assert_eq!(consequences.status.as_deref(), Some("!true  •  exit 0"));
        assert!(consequences.refresh);
        assert!(session.output().is_none());
        let _: &dyn Adapter = &ProcessAdapter;
    }

    #[test]
    fn terminal_and_shell_failures_are_interpreted_inside_the_session() {
        let adapter = MemoryAdapter {
            terminal_result: Mutex::new(Some(Err("missing launcher".to_owned()))),
            ..MemoryAdapter::default()
        };
        let mut session = CommandSession::default();
        session.begin(':');
        session.change("terminal".to_owned());
        let CommandAction::Execute(execution) = session.submit(PathBuf::from("/work")) else {
            panic!("expected terminal execution");
        };
        let consequences = session.complete(Ok(execution.run(&adapter)), Path::new("/work"));

        assert_eq!(consequences, Consequences::default());
        assert!(
            session
                .output()
                .unwrap()
                .detail
                .contains("missing launcher")
        );

        let consequences = session.complete(
            Ok(Completion::Shell(Err(Failure::RequiresTerminal))),
            Path::new("/work"),
        );
        assert_eq!(consequences, Consequences::default());
        assert_eq!(
            session.output().unwrap().summary,
            "interactive terminal required"
        );
    }

    #[test]
    fn stateful_shell_completion_requests_navigation() {
        let mut session = CommandSession::default();
        let consequences = session.complete(
            Ok(Completion::Shell(Ok(ShellReport {
                summary: ":cd /tmp  •  exit 0".to_owned(),
                detail: String::new(),
                final_directory: Some(PathBuf::from("/tmp")),
                successful: true,
            }))),
            Path::new("/work"),
        );

        assert_eq!(consequences.navigate, Some(PathBuf::from("/tmp")));
        assert!(consequences.refresh);
    }
}
