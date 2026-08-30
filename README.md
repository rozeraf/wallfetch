
# Noctalia → Fastfetch logo colors

`wallfetch` takes the current Noctalia wallpaper, extracts an **ordered horizontal palette**, compresses it to the number of Fastfetch logo color slots you want, then launches Fastfetch with true-color `--logo-color-N` overrides.

It is intentionally tuned for wallpapers like the example with several horizontal capsules / stripes / blocks on a flat background. Preserving the **left-to-right order** is the point; this is not a generic photo palette extractor.

## Why this is separate from Noctalia's generated theme

Noctalia's wallpaper theming ultimately reduces the wallpaper to a **single source/seed color** and builds a Material palette from that seed. That is good for UI theming, but it throws away the ordered red→purple→blue progression that we want for a 5-part Fastfetch logo.

`wallfetch` therefore reuses the current wallpaper path, but performs its own multi-color extraction:

1. downsample the wallpaper;
2. estimate the flat background from the border;
3. find the main horizontal foreground band;
4. split that band into 8 source regions and take a robust median color from each;
5. reduce 8 → 5 in OKLab (perceptual color space), preserving the first and last colors;
6. pass the 5 colors to Fastfetch as 24-bit ANSI colors.

## Install (Arch Linux)

```bash
sudo pacman -S python-pillow fastfetch
install -Dm755 wallfetch ~/.local/bin/wallfetch
```

Make sure `~/.local/bin` is in `$PATH`.

## Test on a wallpaper

```bash
wallfetch --wallpaper ~/Pictures/Wallpapers/example.png --print
```

For the supplied screenshot, the extractor returns these 8 source colors:

```text
#ff6f61 #ec6f86 #d66ea5 #b66cb6 #9567c5 #735fd0 #5d57bd #4a4e9f
```

and these 5 Fastfetch colors:

```text
#ff6f61 #db6f9e #a66abe #6e5dcb #4a4e9f
```

Run Fastfetch through the wrapper:

```bash
wallfetch
```

Any extra Fastfetch arguments are forwarded:

```bash
wallfetch --config ~/.config/fastfetch/config.jsonc
```

If you want `fastfetch` in your shell to always use dynamic wallpaper colors, an alias is enough:

```bash
alias fastfetch='wallfetch'
```

The Python process launches the real `fastfetch` executable directly, so the shell alias does not recurse.

## Fastfetch logo requirements

Fastfetch text logos support color slots 1–9. For a custom text logo, use `$1`, `$2`, ... `$5` before the corresponding blocks/symbols. Use `logo.type = "file"` (not `file-raw`) so Fastfetch interprets those placeholders.

Example:

```jsonc
{
  "logo": {
    "type": "file",
    "source": "~/.config/fastfetch/logo.txt"
  }
}
```

Inside `logo.txt`, place the five color placeholders where the five parts begin, for example conceptually:

```text
$1BLOCK1$2BLOCK2$3BLOCK3$4BLOCK4$5BLOCK5
```

If your built-in logo already has five color slots, no logo file change is needed.

## Noctalia integration

### Noctalia v5+

`wallfetch` first tries Noctalia's native IPC (`noctalia msg wallpaper-get`). It also understands `NOCTALIA_WALLPAPER_PATH`, which Noctalia exposes to the `wallpaper_changed` hook.

You do **not** need a hook because `wallfetch` caches by wallpaper path + mtime and recalculates lazily. If you want to pre-warm the cache immediately on wallpaper change, add this to a Noctalia TOML config:

```toml
[hooks]
wallpaper_changed = "~/.local/bin/wallfetch --wallpaper \"$NOCTALIA_WALLPAPER_PATH\" --cache-only"
```

### Noctalia Shell v4 / Quickshell

The wrapper falls back to:

```bash
qs ipc call wallpaper get
```

On a multi-monitor setup, specify the connector if needed:

```bash
wallfetch --monitor DP-1
```

You can also bypass all auto-detection:

```bash
wallfetch --wallpaper /absolute/path/to/wallpaper.png
```

## Reduction modes

Default (`resample`) preserves the two endpoint colors and interpolates the three middle logo colors in OKLab:

```bash
wallfetch --reduction resample --print
```

If you want a literal area-average 8 → 5 reduction instead:

```bash
wallfetch --reduction mean --print
```

On the supplied screenshot, `mean` produces approximately:

```text
#f86f70 #d76fa0 #a66abe #725eca #5151aa
```

## Useful options

```text
--source-count 8       source horizontal color regions
--logo-count 5         Fastfetch output slots (1..9)
--monitor DP-1         Noctalia connector
--background-threshold 0.055
--print                show detected colors and Fastfetch arguments
--json                 machine-readable output
--cache-only           update cache without starting Fastfetch
--no-cache             force re-extraction
```

Cache file:

```text
~/.cache/noctalia-fastfetch/palette.json
```
