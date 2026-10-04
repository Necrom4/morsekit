# morsekit

A terminal Morse code keyer with live translation

## Requirements

Your terminal must support key press and release events. Run it in a compatible
terminal such as kitty; some embedded terminals (neovim, tmux) do not forward
key releases.

## Install

Install from GitHub with Cargo:

```sh
cargo install --git https://github.com/necrom4/morsekit.git
```

Or run it from a clone:

```sh
cargo run --release
```
