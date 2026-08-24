# PolarExp

A deliberately small Iced file explorer written in Rust for Linux desktops.

## Requirements

- Rust 1.92 or newer
- GLib/GIO development files
- X11 or Wayland development files required by Iced's Winit backend

On Debian or Ubuntu, install the GIO and X11 dependencies with:

```sh
sudo apt install \
  libglib2.0-dev \
  libx11-xcb-dev \
  xinput \
  libxcursor-dev \
  libxkbcommon-x11-dev \
  libx11-dev
```

For a Wayland-only environment, also install:

```sh
sudo apt install libwayland-dev libxkbcommon-dev
```

## Packages

Build the regular Linux archive and verify its desktop entry, AppStream metadata, icon, dynamic
libraries, and graphical startup with:

```sh
scripts/package-archive.sh
scripts/smoke-package.sh dist/polarexp-*-linux.tar.gz
```

For a system installation under `/usr`, build as the regular user and install as root:

```sh
make build
sudo make install
```

The installer writes `/usr/bin/polarexp` together with the desktop entry, AppStream metadata, and
icon under `/usr/share`. It refreshes the desktop and icon caches, but does not change MIME defaults
or user configuration. `make install` does not compile as root and fails if the release binary is
missing. Packagers can stage the same file set with `make install DESTDIR=/path/to/package-root`.
Remove only the installed application files with:

```sh
sudo make uninstall
```

The Flatpak manifest is `packaging/io.github.powerpenguini.PolarExp.yml`. It uses frozen Cargo
sources generated from `Cargo.lock`, the Freedesktop 25.08 runtime, and the matching stable Rust
SDK extension. On a machine with Flatpak Builder and Flathub configured, build and launch it with:

```sh
flatpak-builder --force-clean --user --install-deps-from=flathub \
  --install build-flatpak packaging/io.github.powerpenguini.PolarExp.yml
flatpak run io.github.powerpenguini.PolarExp
```

PolarExp is a general local file manager, so the Flatpak deliberately requests host filesystem
access together with Wayland/X11, GVfs, and UDisks access. A Flathub submission therefore needs
the normal host-filesystem exception for this application class. The package workflow builds both
formats on version tags and can also be started manually.

## Run

```sh
cargo run
```

Iced uses WGPU by default and automatically falls back to its software renderer when needed. To
force the software renderer while diagnosing a graphics-driver issue, run:

```sh
ICED_BACKEND=tiny-skia cargo run
```

PolarExp starts in the last visited directory by default. Put `set startup=cwd` in `polarexprc` to start in the directory from which it is launched. An explicit path or local `file:` URI on the command line always wins. Single-click folders to browse, and double-click files to open them with the system default application. Right-click an item to rename it or move it to Trash. Drag files or folders onto a folder in the main grid or sidebar tree to move them there; hold `Ctrl` when releasing to copy instead. If the grabbed item belongs to a multi-item selection, the whole selection is transferred.

On Wayland and X11, the same gesture works between separately launched PolarExp windows and with compatible applications. A PolarExp-to-PolarExp drop moves by default, while `Ctrl` makes it a copy; other applications negotiate the standard action they support. PolarExp also accepts local-file drops from compatible applications. Drop onto a folder tile or sidebar folder, or onto empty grid space to use the currently displayed folder.

Vim-style navigation is available in the browser: `h`, `j`, `k`, and `l` move the selection left, down, up, and right across the file grid. `0` and `$` move it to the first and last item in the current grid row. `Enter` opens the selected item. `Ctrl+O` or netrw-style `u` returns to the previously visited folder. `Ctrl+W`, then `e`, toggles the sidebar tree. These shortcuts are inactive while entering text or responding to a bottom-bar prompt.

Press `r` to rename the selected item in the bottom bar. `Enter` saves the new name, and `Esc` cancels the rename.

New Folder, deletion confirmations, permanent-delete fallback, and errors also use the bottom bar. Name prompts use `Enter` or `Esc`. Deletion prompts use `Y/n`, with `Enter` and `Esc` kept as aliases. `Esc` closes errors. Long failure details expand above the bar without covering the file grid.

Press `v` to start a Vim-style visual selection, then extend it with `h`, `j`, `k`, `l`, `0`, and `$`. Press `v` again to keep the selected range or `Esc` to collapse it to the active item. In Grid view, you can also drag from empty space around the tiles to select every item intersecting the rectangle.

Press `x` to move the current selection to Trash. `d` starts a Vim-style pending delete operator: `dd` uses only the active item, `d0` selects to the beginning of the grid row, `d$` to its end, `dh`/`dl` an adjacent item, and `dk`/`dj` the current and adjacent row. The resulting selection is moved to Trash after one confirmation; `Esc` cancels a pending operator.

Press `/` to jump between matching names in the current folder. Type a second slash and a query, such as `//invoice`, to search recursively from the current folder. Recursive results show paths relative to that folder; `Enter` opens the first result, `Esc` restores the original folder view, and deleting the second slash switches back to the local search.

Press `!` to run an isolated Bash command in the current folder. Commands are not sandboxed: they run with the current user's permissions and can modify every file that account can access. Changing a command's working directory does not move PolarExp. Use the Vim-style `:` prompt for commands that may also change application state: for example, `:cd /tmp` navigates to that directory, `:t` or `:terminal` opens the default terminal there, `:diagnostics` shows the retained local command-failure history, `:help` shows the command and shortcut reference, and `:q` quits PolarExp. Standard output and errors expand in the bottom bar, can be copied, and the folder view is refreshed when a Bash command finishes. Commands that try to take over an interactive terminal screen are stopped and reported in the bottom bar instead of blocking the file browser.

`:set` and `:setlocal` change only the running session. Grid/List, sorting, and other view controls are also session-only. Persistent UI settings come from `$XDG_CONFIG_HOME/polarexp/polarexprc`, or `~/.config/polarexp/polarexprc` when `XDG_CONFIG_HOME` is unset. PolarExp reads this file at startup and never creates or rewrites it. The format uses command-like lines without a leading colon:

```vim
" PolarExp configuration
set view=list sort=name folders-first=true
set tree=true startup=last
setlocal "~/Downloads" view=grid hidden=false
```

Blank lines, comment lines beginning with `"` or `#`, and inline `#` comments are ignored. `setlocal` accepts an absolute path or a path starting with `~/`. Values resolve from defaults through config-wide and session-wide settings to config-local and session-local overrides. A config error rejects the whole file and appears in the bottom bar with its line number. Restart PolarExp after editing the file.

The sidebar lists the computer root and mounted volumes. Its folders are loaded lazily as you expand them. Hide it with `:set tree=false` or `Ctrl+W e`; showing it again preserves the expanded folders and scroll position. You can also type an absolute path, or a path relative to the current folder, into the location field.

## Architecture

- `src/app/mod.rs` defines the Iced adapter seam. Its private implementation is split by runtime input, Navigation and Search coordination, file operations, Transfer coordination, status, views, bottom-bar rendering, and presentation helpers under `src/app/`.
- `src/app/grid.rs` owns file-grid selection and interaction geometry. `navigation.rs` owns the Navigation session across folders, Recent, Trash, and recursive-search display restoration, while `state.rs` and `tree.rs` manage the lazy sidebar hierarchy.
- `src/app/operations.rs` runs blocking work through bounded Tokio lanes and rejects cancelled navigation, details, and search results before they reach the UI.
- `src/app/search.rs` owns each search session. `command.rs` owns command parsing and result policy behind production and in-memory adapters; `shell.rs` is its Bash process adapter.
- `src/app/transfer_session.rs` owns each Transfer session, including queueing, conflicts, Trash restore completion, progress, retry, and retained history. `trash.rs` keeps Trash receipt cleanup, `src/transfer.rs` owns clipboard and drag behavior, and `native_dnd.rs` and `x11_dnd.rs` remain platform adapters. `src/fs/mod.rs` keeps the filesystem seam while private files own browsing, Transfer batch policy, mutations, and metadata-preserving Copy.
- `src/journal.rs` keeps the Journal seam while private files own persistence, fingerprints, action effects, and Trash receipt discovery.
- `src/app/location_monitoring.rs` owns watched-location roles and polling fallback. `directory_watch.rs` is its inotify adapter.
- `src/theme/mod.rs` reads the desktop accent and selection colors.
