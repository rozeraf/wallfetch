use clap::{Parser, ValueEnum};
use image::{imageops::FilterType, RgbImage};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, UNIX_EPOCH};

type Rgb = [u8; 3];
type Lab = [f64; 3];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Reduction {
    Resample,
    Mean,
}

#[derive(Debug, Parser)]
#[command(
    name = "wallfetch",
    about = "Extract ordered wallpaper colors and apply them to Fastfetch logo color slots."
)]
struct Args {
    /// Explicit wallpaper path; otherwise auto-detect Noctalia
    #[arg(long)]
    wallpaper: Option<PathBuf>,

    /// Noctalia output connector, e.g. DP-1
    #[arg(long)]
    monitor: Option<String>,

    /// Source color regions to extract
    #[arg(long, default_value_t = 8)]
    source_count: usize,

    /// Fastfetch logo color slots
    #[arg(long, default_value_t = 5)]
    logo_count: usize,

    /// Color reduction method
    #[arg(long, value_enum, default_value_t = Reduction::Resample)]
    reduction: Reduction,

    /// OKLab distance from the border background
    #[arg(long, default_value_t = 0.055)]
    background_threshold: f64,

    /// Print detected colors and exit
    #[arg(long)]
    print: bool,

    /// Print machine-readable colors and exit
    #[arg(long)]
    json: bool,

    /// Refresh the cache without starting Fastfetch
    #[arg(long)]
    cache_only: bool,

    /// Force color re-extraction
    #[arg(long)]
    no_cache: bool,

    /// Arguments forwarded to Fastfetch
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    fastfetch_args: Vec<OsString>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CacheKey {
    wallpaper: PathBuf,
    mtime_ns: u128,
    size: u64,
    source_count: usize,
    logo_count: usize,
    reduction: Reduction,
    background_threshold: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    key: CacheKey,
    source_colors: Vec<Rgb>,
    logo_colors: Vec<Rgb>,
    source_hex: Vec<String>,
    logo_hex: Vec<String>,
}

fn srgb_to_linear(value: u8) -> f64 {
    let x = f64::from(value) / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn rgb_to_oklab(rgb: Rgb) -> Lab {
    let r = srgb_to_linear(rgb[0]);
    let g = srgb_to_linear(rgb[1]);
    let b = srgb_to_linear(rgb[2]);
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

fn linear_to_srgb(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let x = if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn oklab_to_rgb(lab: Lab) -> Rgb {
    let [l, a, b] = lab;
    let l_root = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_root = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_root = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let l = l_root.powi(3);
    let m = m_root.powi(3);
    let s = s_root.powi(3);
    [
        linear_to_srgb(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        linear_to_srgb(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        linear_to_srgb(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    ]
}

fn lab_distance(a: Lab, b: Lab) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn rgb_hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}
fn ansi_truecolor(rgb: Rgb) -> String {
    format!("38;2;{};{};{}", rgb[0], rgb[1], rgb[2])
}

fn normalize_path(text: &str) -> Option<PathBuf> {
    let mut value = text.trim().trim_matches(['\"', '\'']).to_owned();
    if let Some(rest) = value.strip_prefix("file://") {
        value = urlencoding::decode(rest).ok()?.into_owned();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        value = env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(rest).to_string_lossy().into_owned())?;
    }
    let path = PathBuf::from(value);
    path.is_file()
        .then(|| fs::canonicalize(&path).unwrap_or(path))
}

fn image_from_output(output: &str) -> Option<PathBuf> {
    for line in output.lines().rev() {
        let line = line.trim();
        for candidate in [line, line.split_once(": ").map_or("", |(_, rhs)| rhs)] {
            if let Some(path) = normalize_path(candidate) {
                return Some(path);
            }
        }
    }
    let quoted = Regex::new(r#"[\"']([^\"']+)[\"']"#).unwrap();
    quoted
        .captures_iter(output)
        .filter_map(|cap| normalize_path(&cap[1]))
        .last()
}

fn command_path(program: &str, args: &[&str]) -> Option<PathBuf> {
    let output = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?
        .wait_with_output()
        .ok()?;
    image_from_output(&String::from_utf8_lossy(&output.stdout))
}

fn find_paths_json(value: &JsonValue, context: &str, found: &mut Vec<PathBuf>) {
    match value {
        JsonValue::Object(map) => {
            for (key, value) in map {
                find_paths_json(value, &format!("{context}.{key}"), found);
            }
        }
        JsonValue::Array(values) => {
            for value in values {
                find_paths_json(value, context, found);
            }
        }
        JsonValue::String(value)
            if context.to_ascii_lowercase().contains("wallpaper")
                || context.to_ascii_lowercase().contains("path") =>
        {
            if let Some(path) = normalize_path(value) {
                found.push(path);
            }
        }
        _ => {}
    }
}

fn state_file_wallpaper() -> Option<PathBuf> {
    let state_home = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    for name in ["state.toml", "settings.toml"] {
        let Ok(text) = fs::read_to_string(state_home.join("noctalia").join(name)) else {
            continue;
        };
        if let Ok(value) = text.parse::<toml::Value>() {
            let json = serde_json::to_value(value).ok()?;
            let mut found = Vec::new();
            find_paths_json(&json, "", &mut found);
            if let Some(path) = found.pop() {
                return Some(path);
            }
        }
    }
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
        .join("noctalia");
    for entry in fs::read_dir(config).ok()? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str(&text) else {
            continue;
        };
        let mut found = Vec::new();
        find_paths_json(&value, "", &mut found);
        if let Some(path) = found.pop() {
            return Some(path);
        }
    }
    None
}

fn detect_wallpaper(args: &Args) -> Result<PathBuf, String> {
    if let Some(path) = &args.wallpaper {
        return normalize_path(&path.to_string_lossy())
            .ok_or_else(|| format!("wallpaper does not exist: {}", path.display()));
    }
    if let Ok(path) = env::var("NOCTALIA_WALLPAPER_PATH") {
        if let Some(path) = normalize_path(&path) {
            return Ok(path);
        }
    }
    let monitor = args.monitor.as_deref();
    let mut noctalia = vec!["msg", "wallpaper-get"];
    if let Some(value) = monitor {
        noctalia.push(value);
    }
    if let Some(path) = command_path("noctalia", &noctalia) {
        return Ok(path);
    }
    let mut qs = vec!["ipc", "call", "wallpaper", "get"];
    if let Some(value) = monitor {
        qs.push(value);
    }
    command_path("qs", &qs)
        .or_else(state_file_wallpaper)
        .ok_or_else(|| {
            "could not determine the current Noctalia wallpaper; pass --wallpaper /path/to/image"
                .to_owned()
        })
}

fn background_lab(image: &RgbImage) -> Lab {
    let (width, height) = image.dimensions();
    let border_width = 2.max(width.min(height) / 28);
    let mut buckets: HashMap<Rgb, usize> = HashMap::new();
    let mut border = Vec::new();
    for (x, y, pixel) in image.enumerate_pixels() {
        if x < border_width
            || x >= width - border_width
            || y < border_width
            || y >= height - border_width
        {
            let rgb = pixel.0;
            let key = [rgb[0] / 16, rgb[1] / 16, rgb[2] / 16];
            *buckets.entry(key).or_default() += 1;
            border.push((rgb, key));
        }
    }
    let mode = buckets
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .unwrap()
        .0;
    let labs: Vec<_> = border
        .into_iter()
        .filter(|(_, key)| *key == mode)
        .map(|(rgb, _)| rgb_to_oklab(rgb))
        .collect();
    let mut result = [0.0; 3];
    for lab in &labs {
        for i in 0..3 {
            result[i] += lab[i];
        }
    }
    for value in &mut result {
        *value /= labs.len() as f64;
    }
    result
}

fn longest_run(flags: &[bool]) -> Option<(usize, usize)> {
    let (mut best, mut start) = (None, None);
    for i in 0..=flags.len() {
        let value = flags.get(i).copied().unwrap_or(false);
        match (value, start) {
            (true, None) => start = Some(i),
            (false, Some(begin)) => {
                let end = i - 1;
                if best.is_none_or(|(a, b)| end - begin > b - a) {
                    best = Some((begin, end));
                }
                start = None;
            }
            _ => {}
        }
    }
    best
}

fn median(values: &mut [u8]) -> u8 {
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        (u16::from(values[middle - 1]) + u16::from(values[middle])).div_ceil(2) as u8
    }
}

fn extract_horizontal_colors(
    path: &Path,
    count: usize,
    threshold: f64,
) -> Result<Vec<Rgb>, String> {
    let image = image::open(path)
        .map_err(|e| e.to_string())?
        .resize_exact(224, 112, FilterType::Triangle)
        .to_rgb8();
    let (width, height) = image.dimensions();
    let background = background_lab(&image);
    let mut columns = vec![Vec::new(); width as usize];
    for (x, _, pixel) in image.enumerate_pixels() {
        if lab_distance(rgb_to_oklab(pixel.0), background) > threshold {
            columns[x as usize].push(pixel.0);
        }
    }
    let minimum = 3.max((f64::from(height) * 0.08).round() as usize);
    let (xmin, xmax) = longest_run(
        &columns
            .iter()
            .map(|c| c.len() >= minimum)
            .collect::<Vec<_>>(),
    )
    .filter(|(a, b)| b - a + 1 >= count)
    .ok_or("could not find one sufficiently large horizontal foreground region")?;
    let span = xmax - xmin + 1;
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let left = xmin as f64 + span as f64 * i as f64 / count as f64;
        let right = xmin as f64 + span as f64 * (i + 1) as f64 / count as f64;
        let middle = (left + right) / 2.0;
        let half = (right - left) * 0.28;
        let mut xa = (middle - half).floor().max(xmin as f64) as usize;
        let mut xb = (middle + half).ceil().min(xmax as f64) as usize;
        let collect = |a: usize, b: usize| {
            columns[a..=b]
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<Rgb>>()
        };
        let mut values = collect(xa, xb);
        if values.len() < 8 {
            xa = left.floor().max(xmin as f64) as usize;
            xb = right.ceil().min(xmax as f64) as usize;
            values = collect(xa, xb);
        }
        if values.is_empty() {
            return Err(format!(
                "source color slice {} contained no foreground pixels",
                i + 1
            ));
        }
        let mut r = values.iter().map(|v| v[0]).collect::<Vec<_>>();
        let mut g = values.iter().map(|v| v[1]).collect::<Vec<_>>();
        let mut b = values.iter().map(|v| v[2]).collect::<Vec<_>>();
        result.push([median(&mut r), median(&mut g), median(&mut b)]);
    }
    Ok(result)
}

fn reduce_colors(colors: &[Rgb], count: usize, method: Reduction) -> Vec<Rgb> {
    let labs: Vec<_> = colors.iter().copied().map(rgb_to_oklab).collect();
    if count == 1 {
        let mut average = [0.0; 3];
        for lab in &labs {
            for i in 0..3 {
                average[i] += lab[i] / labs.len() as f64;
            }
        }
        return vec![oklab_to_rgb(average)];
    }
    if method == Reduction::Mean {
        let scale = labs.len() as f64 / count as f64;
        return (0..count)
            .map(|j| {
                let (a, b) = (j as f64 * scale, (j + 1) as f64 * scale);
                let (mut total, mut weight) = ([0.0; 3], 0.0);
                for (i, lab) in labs.iter().enumerate() {
                    let overlap = (b.min(i as f64 + 1.0) - a.max(i as f64)).max(0.0);
                    if overlap > 0.0 {
                        for c in 0..3 {
                            total[c] += lab[c] * overlap;
                        }
                        weight += overlap;
                    }
                }
                oklab_to_rgb(total.map(|value| value / weight))
            })
            .collect();
    }
    (0..count)
        .map(|j| {
            let position = j as f64 * (labs.len() - 1) as f64 / (count - 1) as f64;
            let i = position.floor() as usize;
            let t = position - i as f64;
            if i >= labs.len() - 1 {
                oklab_to_rgb(labs[labs.len() - 1])
            } else {
                oklab_to_rgb(std::array::from_fn(|c| {
                    labs[i][c] * (1.0 - t) + labs[i + 1][c] * t
                }))
            }
        })
        .collect()
}

fn cache_path() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(env::temp_dir)
        .join("noctalia-fastfetch/palette.json")
}

fn cache_key(wallpaper: &Path, args: &Args) -> io::Result<CacheKey> {
    let metadata = fs::metadata(wallpaper)?;
    let mtime_ns = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    Ok(CacheKey {
        wallpaper: wallpaper.to_owned(),
        mtime_ns,
        size: metadata.len(),
        source_count: args.source_count,
        logo_count: args.logo_count,
        reduction: args.reduction,
        background_threshold: args.background_threshold,
    })
}

fn load_cache(key: &CacheKey) -> Option<(Vec<Rgb>, Vec<Rgb>)> {
    let cache: Cache = serde_json::from_str(&fs::read_to_string(cache_path()).ok()?).ok()?;
    (cache.key == *key).then_some((cache.source_colors, cache.logo_colors))
}

fn save_cache(key: CacheKey, source: &[Rgb], logo: &[Rgb]) -> io::Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cache = Cache {
        key,
        source_colors: source.to_vec(),
        logo_colors: logo.to_vec(),
        source_hex: source.iter().copied().map(rgb_hex).collect(),
        logo_hex: logo.iter().copied().map(rgb_hex).collect(),
    };
    let temporary = path.with_extension("tmp");
    fs::write(
        &temporary,
        format!("{}\n", serde_json::to_string_pretty(&cache)?),
    )?;
    fs::rename(temporary, path)
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    if args.source_count == 0 {
        return Err("--source-count must be at least 1".to_owned());
    }
    if !(1..=9).contains(&args.logo_count) {
        return Err("--logo-count must be between 1 and 9".to_owned());
    }
    let wallpaper = detect_wallpaper(&args)?;
    let key = cache_key(&wallpaper, &args).map_err(|e| e.to_string())?;
    let (source, logo) = if !args.no_cache {
        load_cache(&key)
    } else {
        None
    }
    .unwrap_or_else(|| {
        let source =
            extract_horizontal_colors(&wallpaper, args.source_count, args.background_threshold)
                .unwrap_or_else(|e| {
                    eprintln!("wallfetch: color extraction failed: {e}");
                    std::process::exit(2)
                });
        let logo = reduce_colors(&source, args.logo_count, args.reduction);
        if let Err(error) = save_cache(key, &source, &logo) {
            eprintln!("wallfetch: could not save cache: {error}");
        }
        (source, logo)
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "wallpaper": wallpaper, "source": source.iter().copied().map(rgb_hex).collect::<Vec<_>>(),
            "logo": logo.iter().copied().map(rgb_hex).collect::<Vec<_>>(),
            "fastfetch": logo.iter().enumerate().map(|(i, &c)| ((i + 1).to_string(), ansi_truecolor(c))).collect::<HashMap<_, _>>()
        })).unwrap());
        return Ok(());
    }
    if args.print {
        println!("wallpaper: {}", wallpaper.display());
        println!(
            "source: {}",
            source
                .iter()
                .copied()
                .map(rgb_hex)
                .collect::<Vec<_>>()
                .join("  ")
        );
        println!(
            "logo:   {}",
            logo.iter()
                .copied()
                .map(rgb_hex)
                .collect::<Vec<_>>()
                .join("  ")
        );
        println!(
            "fastfetch args:\n  {}",
            logo.iter()
                .enumerate()
                .map(|(i, &c)| format!("--logo-color-{} '{}'", i + 1, ansi_truecolor(c)))
                .collect::<Vec<_>>()
                .join(" ")
        );
        return Ok(());
    }
    if args.cache_only {
        return Ok(());
    }
    let mut command = Command::new("fastfetch");
    for (i, &color) in logo.iter().enumerate() {
        command.args([format!("--logo-color-{}", i + 1), ansi_truecolor(color)]);
    }
    let error = command.args(args.fastfetch_args).exec();
    Err(if error.kind() == io::ErrorKind::NotFound {
        "fastfetch was not found in PATH".to_owned()
    } else {
        error.to_string()
    })
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
    fn oklab_roundtrip() {
        for color in [[0, 0, 0], [255, 255, 255], [255, 111, 97], [74, 78, 159]] {
            assert_eq!(oklab_to_rgb(rgb_to_oklab(color)), color);
        }
    }

    #[test]
    fn resample_preserves_endpoints() {
        let colors = [[255, 0, 0], [128, 0, 128], [0, 0, 255]];
        let reduced = reduce_colors(&colors, 2, Reduction::Resample);
        assert_eq!(reduced, vec![[255, 0, 0], [0, 0, 255]]);
    }
}
