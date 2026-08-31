use crate::palette::Reduction;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperSource {
    #[default]
    Noctalia,
    Path,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct WallpaperConfig {
    pub source: WallpaperSource,
    pub path: Option<PathBuf>,
    pub monitor: Option<String>,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            source: WallpaperSource::Noctalia,
            path: None,
            monitor: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct PaletteConfig {
    pub source_count: usize,
    pub color_count: usize,
    pub reduction: Reduction,
    pub background_threshold: f64,
}

impl Default for PaletteConfig {
    fn default() -> Self {
        Self {
            source_count: 8,
            color_count: 5,
            reduction: Reduction::Resample,
            background_threshold: 0.055,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TemplateConfig {
    pub path: Option<PathBuf>,
    pub inline: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub wallpaper: WallpaperConfig,
    pub palette: PaletteConfig,
    pub template: TemplateConfig,
}

pub fn config_home() -> Result<PathBuf, String> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("neither XDG_CONFIG_HOME nor HOME is set".to_owned())
}

pub fn default_config_path() -> Result<PathBuf, String> {
    Ok(config_home()?.join("wallfetch/config.toml"))
}

pub fn expand_path(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_string_lossy();
    let expanded = if let Some(rest) = text.strip_prefix("~/") {
        env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .ok_or("HOME is not set, so ~ cannot be expanded")?
    } else {
        path.to_owned()
    };
    Ok(expanded)
}

pub fn load(explicit: Option<&Path>) -> Result<(Config, Option<PathBuf>), String> {
    let path = match explicit {
        Some(path) => Some(expand_path(path)?),
        None => {
            let default = default_config_path()?;
            default.is_file().then_some(default)
        }
    };
    let Some(path) = path else {
        return Ok((Config::default(), None));
    };
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("could not read config {}: {error}", path.display()))?;
    let config = toml::from_str(&text)
        .map_err(|error| format!("could not parse config {}: {error}", path.display()))?;
    Ok((config, Some(path)))
}

pub fn load_template(
    config: &TemplateConfig,
    override_path: Option<&Path>,
) -> Result<String, String> {
    if let Some(path) = override_path {
        return read_template(path);
    }
    match (&config.path, &config.inline) {
        (Some(_), Some(_)) => Err("template.path and template.inline are mutually exclusive".to_owned()),
        (Some(path), None) => read_template(path),
        (None, Some(template)) => Ok(template.clone()),
        (None, None) => Err(
            "no template configured; set template.path or template.inline in config.toml, or pass --template"
                .to_owned(),
        ),
    }
}

fn read_template(path: &Path) -> Result<String, String> {
    let path = expand_path(path)?;
    fs::read_to_string(&path)
        .map_err(|error| format!("could not read template {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_stable() {
        let config = Config::default();
        assert_eq!(config.wallpaper.source, WallpaperSource::Noctalia);
        assert_eq!(config.palette.source_count, 8);
        assert_eq!(config.palette.color_count, 5);
    }

    #[test]
    fn parses_inline_template() {
        let config: Config = toml::from_str(
            r#"
                [wallpaper]
                source = "path"
                path = "/tmp/wallpaper.png"

                [template]
                inline = "$1one $2two"
            "#,
        )
        .unwrap();
        assert_eq!(config.wallpaper.source, WallpaperSource::Path);
        assert_eq!(config.template.inline.as_deref(), Some("$1one $2two"));
    }
}
