# PolarExp

A deliberately small Slint file explorer written in Rust for Linux desktops.

## Requirements

- Rust 1.92 or newer
- GLib/GIO development files
- X11 or Wayland development files required by Slint's Winit backend

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

PolarExp starts in the directory from which it is launched. Single-click folders to browse, and double-click files to open them with the system default application. Right-click an item to rename it or move it to Trash. Drag files or folders onto a folder in the main grid or sidebar tree to move them there.

Vim-style navigation is available in the browser: `h`, `j`, `k`, and `l` move the selection left, down, up, and right across the file grid. `Enter` opens the selected item. `Ctrl+O` or netrw-style `u` returns to the previously visited folder. These shortcuts are inactive while entering text or using a dialog.

Press `v` to start a Vim-style visual selection, then extend it with `h`, `j`, `k`, and `l`. Press `v` again to keep the selected range or `Esc` to collapse it to the active item. In Grid view, you can also drag from empty space around the tiles to select every item intersecting the rectangle.

Press `x` to move the current selection to Trash. `d` starts a Vim-style pending delete operator: `d0` selects to the beginning of the grid row, `d$` to its end, `dd` the whole row, `dh`/`dl` an adjacent item, and `dk`/`dj` the current and adjacent row. The resulting selection is moved to Trash after one confirmation; `Esc` cancels a pending operator.

Press `/` to jump between matching names in the current folder. Type a second slash and a query, such as `//invoice`, to search recursively from the current folder. Recursive results show paths relative to that folder; `Enter` opens the first result, `Esc` restores the original folder view, and deleting the second slash switches back to the local search.

Press `!` to run an isolated Bash command in the current folder. Bash can modify files, but changing its working directory does not move PolarExp. Use the Vim-style `:` prompt for commands that may also change application state: for example, `:cd /tmp` navigates to that directory and `:q` quits PolarExp. Standard output and errors expand in the bottom bar, and the folder view is refreshed when a Bash command finishes. Commands that try to take over an interactive terminal screen are stopped and reported in the bottom bar instead of blocking the file browser.

The sidebar lists the computer root and mounted volumes. Its folders are loaded lazily as you expand them. You can also type an absolute path—or a path relative to the current folder—into the location field.

## Architecture

- `src/app/explorer.rs` owns the application lifecycle and connects Slint callbacks. Feature-specific `Explorer` implementations live under `src/app/explorer/`.
- `src/app/state.rs` contains UI-independent navigation and selection state. `tree.rs` manages sidebar data, while `view.rs` maps Rust state into Slint models.
- Filesystem work runs through a small fixed-size executor in `src/app/executor.rs`; results return to the Slint event loop before state or UI updates.
- `src/ui/app-window.slint` defines the Rust-facing UI contract. The visual areas are composed from the components in `src/ui/components/`.
