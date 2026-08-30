# Noctalia → Fastfetch logo colors

`wallfetch` colors a Fastfetch text logo directly from the current Noctalia wallpaper. It preserves the wallpaper's left-to-right color order instead of using Noctalia's single Material seed color.

The extractor is intended for wallpapers with horizontal capsules, stripes, or blocks on a flat background; it is not a generic photo palette extractor.

## How it works

1. Finds the current wallpaper from Noctalia state, environment, or IPC.
2. Estimates the flat background and locates the main horizontal foreground band.
3. Extracts 8 ordered median colors and reduces them to 5 colors in OKLab.
4. Replaces `$1` … `$5` in the logo template with true-color ANSI sequences.
5. Stores the rendered logo and metadata in `/tmp/wallfetch-UID/`.
6. On later runs, compares a BLAKE3 hash of the wallpaper, logo template, and extraction settings. If it matches, the image is not decoded again.

The generated ANSI logo is designed to be called from a Fastfetch `command` module. This bypasses Noctalia's generated `logo.color` values completely.

## Build and install

Requirements: a current stable Rust toolchain and Fastfetch.

```bash
sudo pacman -S rust fastfetch
cargo build --release
install -Dm755 target/release/wallfetch ~/.local/bin/wallfetch
```

Make sure `~/.local/bin` is in `$PATH`.

## Logo template

Create `~/.config/fastfetch/logo` and mark the five color regions:

```text
$1BLOCK1 $2BLOCK2 $3BLOCK3 $4BLOCK4 $5BLOCK5
```

The template can be changed with `--logo-template /path/to/logo`.

## Fastfetch configuration

Disable the built-in logo and put the dynamic logo command first in `modules`:

```jsonc
{
  "logo": {
    "type": "none"
  },
  "modules": [
    {
      "type": "command",
      "key": " ",
      "text": "~/.local/bin/wallfetch --render-logo",
      "format": "{result}"
    },
    {
      "type": "title"
    }
  ]
}
```

Now ordinary `fastfetch` calls `wallfetch` only for the logo. No alias or wrapper is required.

## Usage

```bash
# Output the ANSI logo for Fastfetch
wallfetch --render-logo

# Inspect extracted colors
wallfetch --print

# Machine-readable palette
wallfetch --json

# Test a specific wallpaper
wallfetch --wallpaper ~/Pictures/Wallpapers/example.png --render-logo

# Force regeneration
wallfetch --no-cache --render-logo
```

Calling `wallfetch` without an output option remains supported: it launches Fastfetch with dynamic `--logo-color-N` arguments for compatibility.

## Wallpaper discovery

Discovery order:

1. `--wallpaper /path/to/image`;
2. `NOCTALIA_WALLPAPER_PATH`;
3. Noctalia state/config files;
4. Noctalia v5 IPC: `noctalia msg wallpaper-get`;
5. legacy Noctalia v4 / Quickshell IPC: `qs ipc call wallpaper get`.

For multiple monitors, pass `--monitor DP-1`. An explicit `--wallpaper` always takes priority.

## Cache

The per-user cache is stored under:

```text
/tmp/wallfetch-UID/cache.json
/tmp/wallfetch-UID/logo.ansi
```

Changing the wallpaper bytes, template, color counts, reduction mode, or background threshold invalidates it automatically. `/tmp` normally clears on reboot, so the first Fastfetch run in a new session regenerates the logo.

## Main options

```text
--render-logo              print cached/generated ANSI logo
--logo-template PATH       custom logo template
--source-count 8           source horizontal color regions
--logo-count 5             output colors (1..9)
--reduction resample       endpoint-preserving OKLab interpolation
--reduction mean           area-weighted OKLab reduction
--monitor DP-1             Noctalia connector
--background-threshold N   background separation threshold
--print                     print detected colors
--json                      machine-readable output
--cache-only                regenerate cache without output
--no-cache                  force regeneration
```

Run `wallfetch --help` for the complete CLI reference.
