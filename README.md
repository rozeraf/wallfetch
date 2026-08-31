# wallfetch

`wallfetch` is an independent ordered-palette provider for wallpapers. It separates color extraction from output formatting, so scripts and applications can consume the palette directly or apply it to a text template.

It is designed for wallpapers with horizontal capsules, stripes, or blocks on a flat background. Preserving left-to-right color order is the goal; it is not a general-purpose photographic palette extractor.

## Pipeline

```text
wallpaper source
      │
      ▼
ordered source colors → reduced output palette → wallfetch colors
                                      │
                                      └── + template → wallfetch render
```

The two public stages are explicit:

- `wallfetch colors` returns colors without knowing or caring how they will be used;
- `wallfetch render` applies those same colors to `$1`, `$2`, … placeholders in a template.

There is no dependency on a particular output program.

## Build and install

Supported image formats are PNG, JPEG, and WebP.

```bash
cargo build --release
make PREFIX="$HOME/.local" install
```

Or install system-wide:

```bash
sudo make install
```

## Configuration

The default config path is:

```text
$XDG_CONFIG_HOME/wallfetch/config.toml
```

with `~/.config/wallfetch/config.toml` as the usual fallback.

Example:

```toml
[wallpaper]
# "noctalia" discovers the current wallpaper automatically.
# "path" reads the explicit path below.
source = "noctalia"
# path = "~/Pictures/Wallpapers/example.png"
# monitor = "DP-1"

[palette]
source_count = 8
color_count = 5
reduction = "resample"
background_threshold = 0.055

[template]
path = "~/.config/wallfetch/template"
```

Copy the examples to get started:

```bash
mkdir -p ~/.config/wallfetch
cp examples/config.toml ~/.config/wallfetch/config.toml
cp examples/template ~/.config/wallfetch/template
```

### Wallpaper sources

Use the current Noctalia wallpaper:

```toml
[wallpaper]
source = "noctalia"
```

Or use a fixed image:

```toml
[wallpaper]
source = "path"
path = "~/Pictures/Wallpapers/example.png"
```

`--wallpaper /path/to/image.png` overrides either source from the command line.

### Templates

A template may come from a file:

```toml
[template]
path = "~/.config/wallfetch/template"
```

or be embedded in the config:

```toml
[template]
inline = """
$1ONE $2TWO $3THREE $4FOUR $5FIVE
"""
```

`path` and `inline` are mutually exclusive. `wallfetch render` replaces placeholders with ANSI true-color sequences and appends a terminal reset sequence.

## Getting colors

Plain hexadecimal output is stable and easy to consume from shell scripts:

```bash
wallfetch colors
```

```text
#ff6f61 #db6f9e #a66abe #6d5dcb #4a4e9f
```

Other representations:

```bash
wallfetch colors --format rgb
wallfetch colors --format json
```

JSON includes the wallpaper path, the original spatial samples, and the reduced palette:

```json
{
  "wallpaper": "/path/to/wallpaper.png",
  "source": ["#ff6f61", "#ec6f86"],
  "colors": ["#ff6f61", "#db6f9e"]
}
```

This stage does not load or validate a template.

## Rendering a template

```bash
wallfetch render
```

Override the configured template when needed:

```bash
wallfetch render --template /path/to/template
```

The formatted ANSI output is written to stdout. Any application capable of executing a command and preserving ANSI escape sequences can consume it.

The formatting stage can also accept a palette without reading any wallpaper:

```bash
wallfetch render --colors "#ff0000 #00ff00 #0000ff"
```

Use `-` to compose the two stages through a pipe:

```bash
wallfetch colors | wallfetch render --colors -
```

In this form, `render` does not resolve a wallpaper, inspect Noctalia, decode an image, or access the palette cache. It only loads the template and formats the supplied colors.

## Cache

Only palette extraction is cached. Formatting is deliberately separate and inexpensive, so templates can change without re-decoding the wallpaper.

The cache is stored in:

```text
$XDG_RUNTIME_DIR/wallfetch/palette.json
```

If `XDG_RUNTIME_DIR` is unavailable, the fallback is `/tmp/wallfetch-UID/palette.json`.

The BLAKE3 cache key includes wallpaper contents and every extraction setting. Use `--no-cache` to force extraction:

```bash
wallfetch --no-cache colors
```

## CLI overrides

Global options work before or after the subcommand:

```text
--config PATH
--wallpaper PATH
--monitor NAME
--source-count N
--color-count N
--reduction resample|mean
--background-threshold N
--no-cache
```

Run `wallfetch --help`, `wallfetch colors --help`, or `wallfetch render --help` for the complete reference.

## Migration from 0.1

Version 0.2 removes all consumer-specific behavior:

```text
wallfetch --print          → wallfetch colors
wallfetch --json           → wallfetch colors --format json
wallfetch --render-logo    → wallfetch render
--logo-count N             → --color-count N
```

Move the template and settings into `~/.config/wallfetch/`. Programs consuming the formatted output should execute `wallfetch render` through `PATH`.

See [CHANGELOG.md](CHANGELOG.md) for the complete list of breaking changes.

## License

MIT. See [LICENSE](LICENSE).
