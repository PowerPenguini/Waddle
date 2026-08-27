<p align="center">
  <img
    src="data/icons/hicolor/scalable/apps/io.github.powerpenguini.Waddle.svg"
    width="128"
    height="128"
    alt="Waddle logo"
  >
</p>

<h1 align="center">Waddle</h1>

<p align="center">
  <strong>A Linux file manager for the mouse, the keyboard, and everything in between.</strong>
</p>

<p align="center">
  Wayland &nbsp;·&nbsp; X11 &nbsp;·&nbsp; Rust &nbsp;·&nbsp; Iced
</p>

Waddle feels like a familiar desktop file manager until you put your hands on the keyboard. Then
it gives you Vim-style movement, selection, search, commands, Undo, and Redo without taking away
clicks, context menus, drag and drop, or standard shortcuts.

It is built for local Linux files and takes file safety seriously. Copying or moving many items
shows progress and conflicts. Partial failures remain visible. Undo refuses to overwrite changes
made after the original operation.

> Waddle is currently a 0.0.2 preview. Most of the planned 1.0 experience is implemented, but
> compatibility and package testing still need to finish before the first stable release.

## Why Waddle

- Use Grid or List view, sort files naturally, browse hidden files, and preview images.
- Navigate with a mouse, arrow keys, or Vim-style motions. Switch between them whenever you want.
- Copy, Cut, Paste, drag, restore from Trash, cancel long work, and retry failed items.
- Search the current folder or everything below it without leaving the window.
- Browse Favorites, Recent files, Trash, and mounted drives from the Sidebar.
- Keep up to 100 recent operations available for safe Undo and Redo, including after a restart.
- Run local commands or open a terminal in the current folder.
- Keep your activity private. Waddle has no telemetry and does not upload operation history.

The full product direction lives in the [roadmap](ROADMAP.md).

## Try it

Waddle currently builds from source with Rust 1.92 or newer:

```sh
cargo run
cargo run -- ~/Downloads
```

<details>
<summary>Debian and Ubuntu build dependencies</summary>

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

</details>

If a graphics-driver problem prevents startup, force software rendering with:

```sh
ICED_BACKEND=tiny-skia cargo run
```

## Keyboard at a glance

Waddle also supports standard arrows, `Ctrl+C`, `Ctrl+V`, Tab navigation, and the usual mouse
controls.

| Input | Action |
| --- | --- |
| `h`, `j`, `k`, `l` | Move through files or the focused Sidebar |
| `Enter`, `Backspace` | Open the selected item or its parent folder |
| `v` | Start or finish a Visual selection |
| `y`, `x`, `p` | Copy, Cut, or Paste |
| `Delete` | Move the selection to Trash |
| `u`, `Ctrl+R` | Undo or Redo |
| `/query`, `//query` | Search here or search recursively |
| `Ctrl+L`, `Ctrl+H` | Edit the location or show hidden files |
| `Ctrl+O`, `Ctrl+I` | Go Back or Forward |
| `Ctrl+W e` | Show or hide the Sidebar |
| `:`, `!` | Open a Waddle command or run a Bash command |
| `Esc` | Cancel the current action |

Counts, motions, Cut operators, viewport jumps, and more are available through the same keyboard
grammar. Run `:help` inside Waddle for the complete reference.

## Make it yours

Common view and sorting controls are available directly in the interface. Advanced options use
`:set` and `:setlocal`.

To keep settings between launches, create
`$XDG_CONFIG_HOME/waddle/waddlerc`, or `~/.config/waddle/waddlerc` when
`XDG_CONFIG_HOME` is unset:

```vim
" Waddle configuration
set view=list sort=name
set tree=true startup=last file-click=double folder-click=single
setlocal "~/Downloads" view=grid hidden=false
```

Waddle only reads this file. It never creates or rewrites it.
The legacy `click=single|double` option remains accepted and applies the same behavior to files and
folders.

Commands entered with `!` run through Bash with your user permissions. They are not sandboxed and
can change any file your account can access.

## Install from source

```sh
make build
sudo make install
```

This installs the app, desktop entry, metadata, and icon under `/usr`. Remove those files with:

```sh
sudo make uninstall
```

Release archives and Flatpak builds are part of the 1.0 release work. Packaging details and the
remaining hands-on checks live in the [release checklist](docs/release-checklist.md).

## Get involved

Contributions are especially useful for:

- testing clipboard and drag and drop behavior on different Wayland and X11 desktops;
- finding file-operation edge cases before they cost somebody data;
- improving keyboard and mouse interaction;
- documentation, packaging, and accessibility checks;
- focused Rust fixes with behavioral tests.

Before preparing a change, read [`AGENTS.md`](AGENTS.md) for the repository workflow and
[`CONTEXT.md`](CONTEXT.md) for Waddle's vocabulary. Planned work and local issues live in the
[roadmap](ROADMAP.md) and [`.scratch/`](.scratch/).

Run the automated checks before submitting a release-sized change:

```sh
scripts/release-gate.sh
```

## Project notes

- [Product roadmap](ROADMAP.md)
- [Domain vocabulary](CONTEXT.md)
- [Release checklist](docs/release-checklist.md)
- [Local issue conventions](docs/agents/issue-tracker.md)

## License

Waddle is licensed under the [MIT License](LICENSE).
