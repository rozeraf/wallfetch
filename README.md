# Noctalia → Fastfetch logo colors

`wallfetch` takes the current Noctalia wallpaper, extracts an **ordered horizontal palette**, compresses it to the number of Fastfetch logo color slots you want, then launches Fastfetch with true-color `--logo-color-N` overrides.

The extractor is tuned for wallpapers with horizontal capsules, stripes, or blocks on a flat background. Preserving left-to-right order is the point; this is not a generic photo palette extractor.

## How it works

1. Downsample the wallpaper.
2. Estimate its flat background from the border.
3. Find the main horizontal foreground band.
4. Split the band into 8 regions and take a robust median color from each.
5. Reduce 8 colors to 5 in OKLab while preserving the endpoints.
6. Pass the colors to Fastfetch as 24-bit ANSI values.

Noctalia's Material theme uses a single seed color, so it cannot retain an ordered red → purple → blue progression. `wallfetch` reads the same wallpaper but performs its own spatial extraction.

## Build and install

Requirements: a current stable Rust toolchain and Fastfetch.

```bash
sudo pacman -S rust fastfetch
cargo build --release
install -Dm755 target/release/wallfetch ~/.local/bin/wallfetch
```

Make sure `~/.local/bin` is in `$PATH`.

## Usage

```bash
# Inspect colors from a specific wallpaper
wallfetch --wallpaper ~/Pictures/Wallpapers/example.png --print

# Run Fastfetch through the wrapper
wallfetch

# Forward additional arguments to Fastfetch
wallfetch --config ~/.config/fastfetch/config.jsonc
```

To make the wrapper the shell default:

```bash
alias fastfetch='wallfetch'
```

The wrapper launches the real `fastfetch` executable directly, so the alias does not recurse.

## Fastfetch logo

Fastfetch text logos support color slots 1–9. In a custom text logo, place `$1`, `$2`, … before the corresponding blocks and use `logo.type = "file"`, not `file-raw`:

```jsonc
{
  "logo": {
    "type": "file",
    "source": "~/.config/fastfetch/logo.txt"
  }
}
```

Example structure:

```text
$1BLOCK1$2BLOCK2$3BLOCK3$4BLOCK4$5BLOCK5
```

## Noctalia integration

Wallpaper discovery is attempted in this order:

1. `--wallpaper /path/to/image`;
2. `NOCTALIA_WALLPAPER_PATH`;
3. Noctalia v5 IPC: `noctalia msg wallpaper-get`;
4. Noctalia v4 / Quickshell IPC: `qs ipc call wallpaper get`;
5. Noctalia state/config files.

For multiple monitors:

```bash
wallfetch --monitor DP-1
```

The palette is cached by wallpaper identity and extraction options. A Noctalia v5 hook can pre-warm it:

```toml
[hooks]
wallpaper_changed = "~/.local/bin/wallfetch --wallpaper \"$NOCTALIA_WALLPAPER_PATH\" --cache-only"
```

Cache: `~/.cache/noctalia-fastfetch/palette.json`.

## Main options

```text
--source-count 8          source horizontal color regions
--logo-count 5            Fastfetch output slots (1..9)
--reduction resample      endpoint-preserving OKLab interpolation
--reduction mean          area-weighted OKLab reduction
--monitor DP-1            Noctalia connector
--background-threshold    background separation threshold (default: 0.055)
--print                    print detected colors
--json                     machine-readable output
--cache-only               update cache without starting Fastfetch
--no-cache                 force re-extraction
```

Run `wallfetch --help` for the complete CLI reference.
