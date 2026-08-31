use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, IsTerminal},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use gio::prelude::FileExt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Target {
    pub(crate) directory: PathBuf,
    pub(crate) selected: Vec<PathBuf>,
}

pub(crate) fn detach_terminal_invocation() -> io::Result<bool> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    detach_if_terminal(
        io::stdout().is_terminal(),
        &arguments,
        &CurrentExecutableLauncher,
    )
}

trait DetachedLauncher {
    fn launch(&self, arguments: &[OsString]) -> io::Result<()>;
}

struct CurrentExecutableLauncher;

impl DetachedLauncher for CurrentExecutableLauncher {
    fn launch(&self, arguments: &[OsString]) -> io::Result<()> {
        spawn_detached(&env::current_exe()?, arguments)
    }
}

fn detach_if_terminal(
    stdout_is_terminal: bool,
    arguments: &[OsString],
    launcher: &impl DetachedLauncher,
) -> io::Result<bool> {
    if !stdout_is_terminal {
        return Ok(false);
    }
    launcher.launch(arguments)?;
    Ok(true)
}

fn spawn_detached(executable: &Path, arguments: &[OsString]) -> io::Result<()> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: `setsid` is async-signal-safe and this closure runs in the child
    // after `fork` and before `exec`, without touching shared Rust state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    command.spawn().map(|_| ())
}

pub(crate) fn location(argument: &OsStr) -> Result<Target, String> {
    let path = local_path(argument)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    if metadata.is_dir() {
        return Ok(Target {
            directory: canonicalize(&path)?,
            selected: Vec::new(),
        });
    }

    item_target(path)
}

pub(crate) fn show_items(
    arguments: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<Vec<Target>, String> {
    let mut targets: Vec<Target> = Vec::new();
    for argument in arguments {
        let path = local_path(argument.as_ref())?;
        fs::metadata(&path)
            .map_err(|error| format!("could not reveal {}: {error}", path.display()))?;
        let target = item_target(path)?;
        if let Some(existing) = targets
            .iter_mut()
            .find(|existing| existing.directory == target.directory)
        {
            for selected in target.selected {
                if !existing.selected.contains(&selected) {
                    existing.selected.push(selected);
                }
            }
        } else {
            targets.push(target);
        }
    }
    if targets.is_empty() {
        return Err("ShowItems requires at least one local file URI".to_owned());
    }
    Ok(targets)
}

fn local_path(argument: &OsStr) -> Result<PathBuf, String> {
    let encoded = argument.to_string_lossy();
    let path = if encoded.starts_with("file://") {
        if !encoded.starts_with("file:///") {
            return Err("the file URI is not a local path".to_owned());
        }
        gio::File::for_uri(encoded.as_ref())
            .path()
            .ok_or_else(|| "the file URI is not a local path".to_owned())?
    } else {
        let path = PathBuf::from(argument);
        if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(path)
        }
    };
    Ok(path)
}

fn item_target(path: PathBuf) -> Result<Target, String> {
    let Some(name) = path.file_name() else {
        return Ok(Target {
            directory: canonicalize(&path)?,
            selected: Vec::new(),
        });
    };
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let directory = canonicalize(parent)?;
    Ok(Target {
        selected: vec![directory.join(name)],
        directory,
    })
}

fn canonicalize(path: &std::path::Path) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("could not open {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, os::unix::ffi::OsStringExt};

    use super::*;

    #[derive(Default)]
    struct RecordingLauncher {
        launches: RefCell<Vec<Vec<OsString>>>,
    }

    impl DetachedLauncher for RecordingLauncher {
        fn launch(&self, arguments: &[OsString]) -> io::Result<()> {
            self.launches.borrow_mut().push(arguments.to_vec());
            Ok(())
        }
    }

    struct FailingLauncher;

    impl DetachedLauncher for FailingLauncher {
        fn launch(&self, _arguments: &[OsString]) -> io::Result<()> {
            Err(io::Error::other("could not spawn Waddle"))
        }
    }

    #[test]
    fn terminal_invocations_detach_with_unchanged_arguments() {
        let arguments = vec![
            OsString::from("--show-items"),
            OsString::from_vec(b"/tmp/non-utf8-\xff".to_vec()),
        ];
        let launcher = RecordingLauncher::default();

        assert!(detach_if_terminal(true, &arguments, &launcher).unwrap());
        assert_eq!(*launcher.launches.borrow(), vec![arguments]);
    }

    #[test]
    fn non_terminal_invocations_stay_in_the_current_process() {
        let launcher = RecordingLauncher::default();

        assert!(!detach_if_terminal(false, &[], &launcher).unwrap());
        assert!(launcher.launches.borrow().is_empty());
    }

    #[test]
    fn terminal_launch_failures_are_returned_to_the_caller() {
        let error = detach_if_terminal(true, &[], &FailingLauncher).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "could not spawn Waddle");
    }

    #[test]
    fn locations_open_directories_and_reveal_files_without_resolving_their_names() {
        #[cfg(unix)]
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("zażółć file.txt");
        fs::write(&file, "x").unwrap();

        let directory = location(temp.path().as_os_str()).unwrap();
        let revealed = location(gio::File::for_path(&file).uri().as_str().as_ref()).unwrap();

        assert_eq!(directory.directory, temp.path());
        assert!(directory.selected.is_empty());
        assert_eq!(revealed.directory, temp.path());
        assert_eq!(revealed.selected.as_slice(), std::slice::from_ref(&file));

        #[cfg(unix)]
        {
            let link = temp.path().join("download-link");
            symlink(&file, &link).unwrap();
            assert_eq!(location(link.as_os_str()).unwrap().selected, [link]);
        }
    }

    #[test]
    fn show_items_groups_selections_by_parent_and_removes_duplicates() {
        let temp = tempfile::tempdir().unwrap();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        fs::create_dir_all(&first_dir).unwrap();
        fs::create_dir_all(&second_dir).unwrap();
        let one = first_dir.join("one");
        let two = first_dir.join("two");
        let three = second_dir.join("three");
        for path in [&one, &two, &three] {
            fs::write(path, "x").unwrap();
        }

        let targets = show_items([&one, &two, &one, &three]).unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].directory, first_dir);
        assert_eq!(targets[0].selected, [one, two]);
        assert_eq!(targets[1].directory, second_dir);
        assert_eq!(targets[1].selected, [three]);
    }

    #[test]
    fn show_items_rejects_empty_remote_and_missing_inputs() {
        assert_eq!(
            show_items(Vec::<PathBuf>::new()).unwrap_err(),
            "ShowItems requires at least one local file URI"
        );
        assert_eq!(
            show_items(["file://remote/tmp"]).unwrap_err(),
            "the file URI is not a local path"
        );
        assert!(show_items(["/definitely/missing/waddle-item"]).is_err());
    }
}
