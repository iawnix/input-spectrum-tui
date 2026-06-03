# Input Spectrum TUI

`inputspectrum` turns keyboard input into a live spectrum wall. Each key press injects a drifting wave packet into the bands; faster typing produces taller, denser pulses.

The global keyboard listener follows the same broad route as Screenkey: it first tries the X11 Record extension, then falls back to Linux `/dev/input/event*` keyboard events if RECORD is unavailable or rejected. The evdev fallback usually requires running as a user in the `input` group or using `sudo`.

## Run

```bash
cargo run -- --fps 60 --bars 96 --theme cyber
```

## Controls

- `q` / `Esc`: quit
- `space`: pause or resume decay/render updates
- `tab`: switch mode (`bars`, `wave`, `peaks`)
- `1`, `2`, `3`: switch theme (`cyber`, `mono`, `amber`)
- `+` / `-`: adjust sensitivity
## Scope

The visual surface is intentionally clean: no title, status bar, or on-screen control hints. Mouse events are ignored. Terminal-local controls still work when the TUI is focused so the process can be closed safely.
