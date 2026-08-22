use std::path::{Path, PathBuf};

use super::browser_input;
use super::shell::{self, ShellError, ShellReport};

const COMMAND_HELP: &str = "\
Commands
  :help, :h     Show this help
  :terminal, :t Open a terminal in the current directory
  :refresh      Refresh the current view
  :diagnostics  Show local command failure history
  :cd PATH      Change PolarExp's current directory
  :q            Quit PolarExp
  :COMMAND      Run Bash and keep its final directory
  !COMMAND      Run Bash without changing PolarExp's directory";

pub(super) trait Adapter {
    fn execute(&self, current: &Path, prefix: char, command: &str) -> Result<ShellReport, Failure>;

    fn launch_terminal(&self, current: &Path) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ProcessAdapter;

impl Adapter for ProcessAdapter {
    fn execute(&self, current: &Path, prefix: char, command: &str) -> Result<ShellReport, Failure> {
        shell::execute(current, prefix, command).map_err(|error| match error {
            ShellError::RequiresTerminal => Failure::RequiresTerminal,
            ShellError::Io(error) => Failure::Other(error.to_string()),
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

    pub(super) fn run<A: Adapter>(self, adapter: &A) -> Completion {
        match self.kind {
            ExecutionKind::Shell { prefix, command } => {
                Completion::Shell(adapter.execute(&self.current, prefix, &command))
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
pub(super) enum Submission {
    None,
    Quit,
    Updated,
    Refresh,
    Diagnostics,
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

    pub(super) fn submit(&mut self, current: PathBuf) -> Submission {
        let Some(prefix) = self.prefix.take() else {
            return Submission::None;
        };
        let command = std::mem::take(&mut self.text);
        let trimmed = command.trim();
        if prefix == ':' && trimmed == "q" {
            return Submission::Quit;
        }
        if prefix == ':' && matches!(trimmed, "help" | "h") {
            self.output = Some(Output {
                summary: ":help  •  PolarExp commands".to_owned(),
                detail: format!("{COMMAND_HELP}\n\n{}", browser_input::HELP),
            });
            return Submission::Updated;
        }
        if prefix == ':' && matches!(trimmed, "terminal" | "t") {
            return Submission::Execute(Execution {
                current,
                kind: ExecutionKind::Terminal,
            });
        }
        if prefix == ':' && trimmed == "refresh" {
            return Submission::Refresh;
        }
        if prefix == ':' && matches!(trimmed, "diagnostics" | "diag") {
            return Submission::Diagnostics;
        }
        if trimmed.is_empty() {
            return Submission::None;
        }
        Submission::Execute(Execution {
            current,
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
                    detail: "This command tried to take over the terminal screen, so PolarExp stopped it."
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
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct MemoryAdapter {
        calls: Mutex<Vec<String>>,
        shell_result: Mutex<Option<Result<ShellReport, Failure>>>,
        terminal_result: Mutex<Option<Result<(), String>>>,
    }

    impl Adapter for MemoryAdapter {
        fn execute(&self, _: &Path, prefix: char, command: &str) -> Result<ShellReport, Failure> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{prefix}{command}"));
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
            Submission::Updated
        ));
        let output = session.output().unwrap();
        assert!(output.summary.starts_with(":help"));
        assert!(output.detail.contains(":terminal, :t"));
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
            Submission::Refresh
        ));
    }

    #[test]
    fn diagnostics_is_a_builtin_command_not_a_shell_process() {
        let mut session = CommandSession::default();
        session.begin(':');
        session.change("diagnostics".to_owned());

        assert!(matches!(
            session.submit(PathBuf::from("/work")),
            Submission::Diagnostics
        ));
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
        let Submission::Execute(execution) = session.submit(PathBuf::from("/work")) else {
            panic!("expected execution");
        };

        let completion = execution.run(&adapter);
        let consequences = session.complete(Ok(completion), Path::new("/work"));

        assert_eq!(adapter.calls.lock().unwrap().as_slice(), ["!true"]);
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
        let Submission::Execute(execution) = session.submit(PathBuf::from("/work")) else {
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
