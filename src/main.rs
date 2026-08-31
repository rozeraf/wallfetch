mod config;
mod format;
mod palette;
mod wallpaper;

use clap::{Parser, Subcommand, ValueEnum};
use config::{Config, PaletteConfig};
use palette::{rgb_hex, Reduction, Rgb};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "wallfetch",
    version,
    about = "Extract ordered wallpaper palettes and format them through templates"
)]
struct Cli {
    /// Config file; defaults to $XDG_CONFIG_HOME/wallfetch/config.toml
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Override the configured wallpaper with a direct image path
    #[arg(long, global = true)]
    wallpaper: Option<PathBuf>,

    /// Override the configured Noctalia monitor connector
    #[arg(long, global = true)]
    monitor: Option<String>,

    /// Override the number of extracted source regions
    #[arg(long, global = true)]
    source_count: Option<usize>,

    /// Override the number of output palette colors
    #[arg(long, global = true)]
    color_count: Option<usize>,

    /// Override the palette reduction method
    #[arg(long, value_enum, global = true)]
    reduction: Option<Reduction>,

    /// Override the OKLab background distance threshold
    #[arg(long, global = true)]
    background_threshold: Option<f64>,

    /// Ignore any cached palette and extract it again
    #[arg(long, global = true)]
    no_cache: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Extract and print colors without applying a template
    Colors {
        /// Output encoding
        #[arg(long, value_enum, default_value_t = ColorFormat::Hex)]
        format: ColorFormat,
    },

    /// Apply extracted colors to a template and print the formatted result
    Render {
        /// Override template.path or template.inline with a template file
        #[arg(long)]
        template: Option<PathBuf>,

        /// Hex colors separated by whitespace, or - to read them from stdin
        #[arg(long)]
        colors: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum ColorFormat {
    #[default]
    Hex,
    Rgb,
    Json,
}

#[derive(Debug, Deserialize, Serialize)]
struct Cache {
    hash: String,
    wallpaper: PathBuf,
    source: Vec<Rgb>,
    colors: Vec<Rgb>,
}

fn runtime_dir() -> PathBuf {
    if let Some(path) = env::var_os("XDG_RUNTIME_DIR") {
        let candidate = PathBuf::from(path).join("wallfetch");
        if fs::create_dir_all(&candidate).is_ok() {
            return candidate;
        }
    }
    let uid = env::var_os("HOME")
        .and_then(|home| fs::metadata(home).ok())
        .map(|metadata| metadata.uid())
        .unwrap_or(0);
    env::temp_dir().join(format!("wallfetch-{uid}"))
}

fn cache_path() -> PathBuf {
    runtime_dir().join("palette.json")
}

fn hash_file(hasher: &mut blake3::Hasher, path: &Path) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(())
}

fn palette_hash(wallpaper: &Path, config: &PaletteConfig) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"wallfetch-palette-v1\0");
    hash_file(&mut hasher, wallpaper)?;
    hasher.update(&config.source_count.to_le_bytes());
    hasher.update(&config.color_count.to_le_bytes());
    hasher.update(&config.background_threshold.to_le_bytes());
    hasher.update(&[config.reduction as u8]);
    Ok(hasher.finalize().to_hex().to_string())
}

fn load_cache(hash: &str) -> Option<Cache> {
    let cache: Cache = serde_json::from_str(&fs::read_to_string(cache_path()).ok()?).ok()?;
    (cache.hash == hash).then_some(cache)
}

fn save_cache(cache: &Cache) -> Result<(), String> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(
        &temporary,
        format!(
            "{}\n",
            serde_json::to_string_pretty(cache).map_err(|error| error.to_string())?
        ),
    )
    .map_err(|error| error.to_string())?;
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn apply_overrides(config: &mut Config, cli: &Cli) -> Result<(), String> {
    if let Some(monitor) = &cli.monitor {
        config.wallpaper.monitor = Some(monitor.clone());
    }
    if let Some(count) = cli.source_count {
        config.palette.source_count = count;
    }
    if let Some(count) = cli.color_count {
        config.palette.color_count = count;
    }
    if let Some(reduction) = cli.reduction {
        config.palette.reduction = reduction;
    }
    if let Some(threshold) = cli.background_threshold {
        config.palette.background_threshold = threshold;
    }
    if config.palette.source_count == 0 {
        return Err("source_count must be at least 1".to_owned());
    }
    if config.palette.color_count == 0 {
        return Err("color_count must be at least 1".to_owned());
    }
    if !config.palette.background_threshold.is_finite()
        || config.palette.background_threshold <= 0.0
    {
        return Err("background_threshold must be a positive finite number".to_owned());
    }
    Ok(())
}

fn obtain_palette(
    wallpaper: &Path,
    config: &PaletteConfig,
    no_cache: bool,
) -> Result<Cache, String> {
    let hash = palette_hash(wallpaper, config).map_err(|error| error.to_string())?;
    if !no_cache {
        if let Some(cache) = load_cache(&hash) {
            return Ok(cache);
        }
    }
    let source = palette::extract(wallpaper, config.source_count, config.background_threshold)?;
    let colors = palette::reduce(&source, config.color_count, config.reduction);
    let cache = Cache {
        hash,
        wallpaper: wallpaper.to_owned(),
        source,
        colors,
    };
    if let Err(error) = save_cache(&cache) {
        eprintln!("wallfetch: warning: could not save palette cache: {error}");
    }
    Ok(cache)
}

fn print_colors(cache: &Cache, format: ColorFormat) -> Result<(), String> {
    match format {
        ColorFormat::Hex => println!(
            "{}",
            cache
                .colors
                .iter()
                .copied()
                .map(rgb_hex)
                .collect::<Vec<_>>()
                .join(" ")
        ),
        ColorFormat::Rgb => println!(
            "{}",
            cache
                .colors
                .iter()
                .map(|color| format!("{},{},{}", color[0], color[1], color[2]))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        ColorFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "wallpaper": cache.wallpaper,
                "source": cache.source.iter().copied().map(rgb_hex).collect::<Vec<_>>(),
                "colors": cache.colors.iter().copied().map(rgb_hex).collect::<Vec<_>>(),
            }))
            .map_err(|error| error.to_string())?
        ),
    }
    Ok(())
}

fn parse_hex_colors(input: &str) -> Result<Vec<Rgb>, String> {
    let mut colors = Vec::new();
    for value in input.split_whitespace() {
        let hex = value.strip_prefix('#').unwrap_or(value);
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("invalid color {value:?}; expected #RRGGBB"));
        }
        colors.push([
            u8::from_str_radix(&hex[0..2], 16).map_err(|error| error.to_string())?,
            u8::from_str_radix(&hex[2..4], 16).map_err(|error| error.to_string())?,
            u8::from_str_radix(&hex[4..6], 16).map_err(|error| error.to_string())?,
        ]);
    }
    if colors.is_empty() {
        return Err("no colors were provided".to_owned());
    }
    Ok(colors)
}

fn supplied_colors(value: &str) -> Result<Vec<Rgb>, String> {
    if value != "-" {
        return parse_hex_colors(value);
    }
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("could not read colors from stdin: {error}"))?;
    parse_hex_colors(&input)
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let (mut config, _) = config::load(cli.config.as_deref())?;
    apply_overrides(&mut config, &cli)?;
    match cli.command {
        Command::Colors { format } => {
            let wallpaper = wallpaper::resolve(&config.wallpaper, cli.wallpaper.as_deref())?;
            let palette = obtain_palette(&wallpaper, &config.palette, cli.no_cache)?;
            print_colors(&palette, format)
        }
        Command::Render { template, colors } => {
            let template = config::load_template(&config.template, template.as_deref())?;
            let colors = match colors {
                Some(value) => supplied_colors(&value)?,
                None => {
                    let wallpaper =
                        wallpaper::resolve(&config.wallpaper, cli.wallpaper.as_deref())?;
                    obtain_palette(&wallpaper, &config.palette, cli.no_cache)?.colors
                }
            };
            let output = format::render_ansi(&template, &colors)?;
            print!("{output}");
            Ok(())
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("wallfetch: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_palette() {
        assert_eq!(
            parse_hex_colors("#ff0000 00ff00  #0000FF").unwrap(),
            vec![[255, 0, 0], [0, 255, 0], [0, 0, 255]]
        );
    }

    #[test]
    fn rejects_invalid_palette() {
        assert!(parse_hex_colors("red").is_err());
        assert!(parse_hex_colors("").is_err());
    }
}
