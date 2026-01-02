# TFM (Terminal File Manager)

Terminal file manager (TUI) with dual-pane navigation and preview.

[![Release](https://img.shields.io/github/v/release/MichaelWeissDEV/TFM)](https://github.com/MichaelWeissDEV/TFM/releases)
[![Pipeline](https://github.com/MichaelWeissDEV/TFM/actions/workflows/release.yml/badge.svg)](https://github.com/MichaelWeissDEV/TFM/actions/workflows/release.yml)
[![License: GPLv3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

Downloads and installable packages are attached to each GitHub Release:
https://github.com/MichaelWeissDEV/TFM/releases

CI pipeline (builds .deb/.rpm/.pkg.tar.zst/AppImage + binaries) runs on tags `vX.Y.Z`.

## Features
- Dual-pane navigation with preview.
- Regex file search (case-insensitive).
- Marker system with search (name/path).
- Open With picker and quick open slots.
- Shell suspend/return (`t` opens a subshell).

## Functions
- Navigate directories and open files with the system default handler.
- Create files/dirs, rename, delete.
- Cut/copy/paste; copy path to clipboard.
- Toggle hidden files and metadata/list columns.
- Preview text/images/binary metadata.

## Config
- Default path: `~/.config/tfm/config.toml` (fallbacks: `~/.tfm.toml`, YAML variants).
- Env override: `TFM_CONFIG=/path/to/config.toml`.
- If no config exists, TFM writes a default one and uses built-in defaults.
- Example config (all defaults): `example_config.toml`.

## Install
### From GitHub Releases (binary)
- Download the `tfm-<version>-x86_64-unknown-linux-gnu.tar.gz` asset.
- Extract and install:
	- `tar -xzf tfm-<version>-x86_64-unknown-linux-gnu.tar.gz`
	- `sudo install -m755 tfm /usr/local/bin/tfm`

### Debian / Ubuntu (.deb)
- Download the `.deb` asset and install:
	- `sudo apt install ./tfm_<version>_amd64.deb`

### Arch Linux (.pkg.tar.zst)
- Download the `.pkg.tar.zst` asset and install:
	- `sudo pacman -U ./tfm-<version>-1-x86_64.pkg.tar.zst`

### openSUSE / RPM (.rpm)
- Download the `.rpm` asset and install:
	- `sudo zypper install ./tfm-<version>-1.x86_64.rpm`

### AppImage
- Download the `.AppImage` asset and run:
	- `chmod +x ./tfm-<version>.AppImage && ./tfm-<version>.AppImage`

## License
GPLv3 (GNU General Public License v3.0 only). See `LICENSE`.

## Theme Colors
Theme colors are strings (named colors or `#RRGGBB`):
- `background`, `foreground`: base UI text/background.
- `accent`: borders/titles.
- `folder`: folder entries.
- `selection_bg`, `selection_fg`: selection highlight.
- `warning`, `error`: warnings/errors (warning is used in preview mismatches).

## Keybinding Format
Each binding is a list of strings:
- Single characters: `"q"`, `"/"`, `"M"`.
- Special keys: `"enter"`, `"esc"`, `"backspace"`, `"up"`, `"down"`, `"left"`, `"right"`.
- Modifiers: `"ctrl+o"` (use uppercase letters for shifted chars, e.g. `"O"`).

## Keybindings (Default)
Normal mode:
- `q`: quit
- `up/k`: move up
- `down/j`: move down
- `left/h`: parent dir
- `right/l/enter`: open entry
- `/`: search (regex)
- `a`: add prefix
- `r`: rename
- `d`: delete prefix
- `m`: set marker
- `M`: marker list
- `g`: jump marker
- `s`: settings prefix
- `v`: view prefix
- `c`: copy (prefix for copy-path)
- `x`: cut
- `p`: paste
- `t`: open shell (exit returns to TFM)
- `o`: open-with quick prefix
- `ctrl+o` or `O`: open-with picker

Add prefix (`a` then):
- `d`: add dir
- any other key: add file (starts input with that key)

Settings prefix (`s` then):
- `r`: toggle permissions (metadata bar)
- `d`: toggle dates (metadata bar)
- `o`: toggle owner (metadata bar)
- `m`: toggle metadata bar
- `h`/`H`: toggle hidden files

View prefix (`v` then):
- `p`: toggle list permissions columns
- `o`: toggle list owner columns

Copy prefix (`c` then):
- `p`: copy selected path to clipboard

Delete prefix (`d` then):
- `d`: confirm delete (then `y/n`)

Marker list (`M`):
- `up/k`, `down/j`: move
- `enter`: jump
- `r`: rename
- `e`: edit path
- `d`: delete
- `a`: add marker
- `/`: search markers (`n:`/`p:` prefixes)
- `esc`: close

Open With picker (`ctrl+o` or `O`):
- type to filter, `backspace` to delete
- `up/down`: move
- `enter`: open
- `esc`: close

## Open With Quick Slots
Configure quick opens in TOML:
```
[open_with]
quick = { 1 = "nvim", 2 = "vim"}
```
Use `o1`, `o2`, `o3` in normal mode. Programs must be in `PATH` or use full paths.

## Marker Search Filters
In marker search (`/` inside marker list), you can scope:
- `n:` or `n/` for name only
- `p:` or `p/` for path only
