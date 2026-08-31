use crate::config::{expand_path, WallpaperConfig, WallpaperSource};
use regex::Regex;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn resolve(config: &WallpaperConfig, override_path: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        return existing_path(path);
    }
    match config.source {
        WallpaperSource::Path => config
            .path
            .as_deref()
            .ok_or("wallpaper.source is path but wallpaper.path is missing".to_owned())
            .and_then(existing_path),
        WallpaperSource::Noctalia => detect_noctalia(config.monitor.as_deref()),
    }
}

fn existing_path(path: &Path) -> Result<PathBuf, String> {
    let path = expand_path(path)?;
    if !path.is_file() {
        return Err(format!("wallpaper does not exist: {}", path.display()));
    }
    Ok(fs::canonicalize(&path).unwrap_or(path))
}

fn normalize_path(text: &str) -> Option<PathBuf> {
    let mut value = text.trim().trim_matches(['\"', '\'']).to_owned();
    if let Some(rest) = value.strip_prefix("file://") {
        value = urlencoding::decode(rest).ok()?.into_owned();
    }
    existing_path(Path::new(&value)).ok()
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
        .filter_map(|capture| normalize_path(&capture[1]))
        .last()
}

fn command_path(program: &str, arguments: &[&str]) -> Option<PathBuf> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().ok()?;
                return image_from_output(&String::from_utf8_lossy(&output.stdout));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
}

fn find_toml_paths(value: &toml::Value, context: &str, found: &mut Vec<PathBuf>) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                find_toml_paths(value, &format!("{context}.{key}"), found);
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                find_toml_paths(value, context, found);
            }
        }
        toml::Value::String(value)
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

fn state_wallpaper() -> Option<PathBuf> {
    let state_home = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    let image_path = Regex::new(r#"[\"'](/[^\"']+\.(?:png|jpe?g|webp))[\"']"#).unwrap();
    for name in ["state.toml", "settings.toml"] {
        let Ok(text) = fs::read_to_string(state_home.join("noctalia").join(name)) else {
            continue;
        };
        if let Ok(value) = text.parse::<toml::Value>() {
            let mut found = Vec::new();
            find_toml_paths(&value, "", &mut found);
            if let Some(path) = found.pop() {
                return Some(path);
            }
        }
        if let Some(path) = image_path
            .captures_iter(&text)
            .filter_map(|capture| normalize_path(&capture[1]))
            .last()
        {
            return Some(path);
        }
    }
    None
}

fn detect_noctalia(monitor: Option<&str>) -> Result<PathBuf, String> {
    if let Ok(path) = env::var("NOCTALIA_WALLPAPER_PATH") {
        if let Some(path) = normalize_path(&path) {
            return Ok(path);
        }
    }
    if let Some(path) = state_wallpaper() {
        return Ok(path);
    }
    let mut arguments = vec!["msg", "wallpaper-get"];
    if let Some(monitor) = monitor {
        arguments.push(monitor);
    }
    command_path("noctalia", &arguments).ok_or_else(|| {
        "could not determine the current Noctalia wallpaper; set wallpaper.path or pass --wallpaper"
            .to_owned()
    })
}
