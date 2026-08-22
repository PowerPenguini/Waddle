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

## Run

```sh
cargo run
```

Iced uses WGPU by default and automatically falls back to its software renderer when needed. To
force the software renderer while diagnosing a graphics-driver issue, run:

```sh
ICED_BACKEND=tiny-skia cargo run
```

PolarExp starts in the directory from which it is launched. Single-click folders to browse, and double-click files to open them with the system default application. Right-click an item to rename it or move it to Trash. Drag files or folders onto a folder in the main grid or sidebar tree to move them there; hold `Ctrl` when releasing to copy instead. If the grabbed item belongs to a multi-item selection, the whole selection is transferred.

On Wayland, the same gesture works between separately launched PolarExp windows and with compatible applications such as file managers and Chrome. A PolarExp-to-PolarExp drop moves by default, while `Ctrl` makes it a copy; other applications negotiate the standard action they support. PolarExp also accepts local-file drops from compatible applications. Drop onto a folder tile or sidebar folder, or onto empty grid space to use the currently displayed folder. External drag-and-drop is Wayland-only; in-window dragging remains available on X11.

Vim-style navigation is available in the browser: `h`, `j`, `k`, and `l` move the selection left, down, up, and right across the file grid. `0` and `$` move it to the first and last item in the current grid row. `Enter` opens the selected item. `Ctrl+O` or netrw-style `u` returns to the previously visited folder. These shortcuts are inactive while entering text or responding to a bottom-bar prompt.

Press `r` to rename the selected item in the bottom bar. `Enter` saves the new name, and `Esc` cancels the rename.

New Folder, deletion confirmations, permanent-delete fallback, and errors also use the bottom bar. Name prompts use `Enter` or `Esc`. Deletion prompts use `Y/n`, with `Enter` and `Esc` kept as aliases. `Esc` closes errors. Long failure details expand above the bar without covering the file grid.

Press `v` to start a Vim-style visual selection, then extend it with `h`, `j`, `k`, `l`, `0`, and `$`. Press `v` again to keep the selected range or `Esc` to collapse it to the active item. In Grid view, you can also drag from empty space around the tiles to select every item intersecting the rectangle.

Press `x` to move the current selection to Trash. `d` starts a Vim-style pending delete operator: `dd` uses only the active item, `d0` selects to the beginning of the grid row, `d$` to its end, `dh`/`dl` an adjacent item, and `dk`/`dj` the current and adjacent row. The resulting selection is moved to Trash after one confirmation; `Esc` cancels a pending operator.

Press `/` to jump between matching names in the current folder. Type a second slash and a query, such as `//invoice`, to search recursively from the current folder. Recursive results show paths relative to that folder; `Enter` opens the first result, `Esc` restores the original folder view, and deleting the second slash switches back to the local search.

Press `!` to run an isolated Bash command in the current folder. Bash can modify files, but changing its working directory does not move PolarExp. Use the Vim-style `:` prompt for commands that may also change application state: for example, `:cd /tmp` navigates to that directory, `:t` or `:terminal` opens the default terminal there, `:help` shows the command and shortcut reference, and `:q` quits PolarExp. Standard output and errors expand in the bottom bar, and the folder view is refreshed when a Bash command finishes. Commands that try to take over an interactive terminal screen are stopped and reported in the bottom bar instead of blocking the file browser.

The sidebar lists the computer root and mounted volumes. Its folders are loaded lazily as you expand them. You can also type an absolute path—or a path relative to the current folder—into the location field.

## Architecture

- `src/app/mod.rs` is the Iced adapter: it owns messages, subscriptions, views, and orchestration between the deeper modules.
- `src/app/grid.rs` owns file-grid selection and interaction geometry. `state.rs` owns navigation state, while `tree.rs` manages the lazy sidebar hierarchy.
- `src/app/operations.rs` runs blocking work through bounded Tokio lanes and rejects cancelled navigation, details, and search results before they reach the UI.
- `src/app/search.rs` owns each search session. `command.rs` owns command parsing and result policy behind production and in-memory adapters; `shell.rs` is its Bash process adapter.
- `src/transfer.rs` owns clipboard, internal-drag, and native-drag transfers. `native_dnd.rs` supplies the Wayland adapter, and `fs/mod.rs` performs filesystem work.
- `src/theme/mod.rs` reads the desktop accent and selection colors.
