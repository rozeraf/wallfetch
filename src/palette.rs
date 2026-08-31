use clap::ValueEnum;
use image::{imageops::FilterType, RgbImage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub type Rgb = [u8; 3];
type Lab = [f64; 3];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Reduction {
    #[default]
    Resample,
    Mean,
}

pub fn rgb_hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

pub fn ansi_truecolor(rgb: Rgb) -> String {
    format!("38;2;{};{};{}", rgb[0], rgb[1], rgb[2])
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
    let l = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lab[0] - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lab[0] - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    [
        linear_to_srgb(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        linear_to_srgb(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        linear_to_srgb(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    ]
}

fn lab_distance(a: Lab, b: Lab) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
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
        .expect("resized image has border pixels")
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

pub fn extract(path: &Path, count: usize, background_threshold: f64) -> Result<Vec<Rgb>, String> {
    let image = image::open(path)
        .map_err(|error| error.to_string())?
        .resize_exact(224, 112, FilterType::Triangle)
        .to_rgb8();
    let (_, height) = image.dimensions();
    let background = background_lab(&image);
    let mut columns = vec![Vec::new(); image.width() as usize];
    for (x, _, pixel) in image.enumerate_pixels() {
        if lab_distance(rgb_to_oklab(pixel.0), background) > background_threshold {
            columns[x as usize].push(pixel.0);
        }
    }
    let minimum = 3.max((f64::from(height) * 0.08).round() as usize);
    let (xmin, xmax) = longest_run(
        &columns
            .iter()
            .map(|column| column.len() >= minimum)
            .collect::<Vec<_>>(),
    )
    .filter(|(start, end)| end - start + 1 >= count)
    .ok_or("could not find a sufficiently large horizontal foreground region")?;
    let span = xmax - xmin + 1;
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let left = xmin as f64 + span as f64 * index as f64 / count as f64;
        let right = xmin as f64 + span as f64 * (index + 1) as f64 / count as f64;
        let middle = (left + right) / 2.0;
        let half = (right - left) * 0.28;
        let mut start = (middle - half).floor().max(xmin as f64) as usize;
        let mut end = (middle + half).ceil().min(xmax as f64) as usize;
        let collect = |a: usize, b: usize| {
            columns[a..=b]
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<Rgb>>()
        };
        let mut values = collect(start, end);
        if values.len() < 8 {
            start = left.floor().max(xmin as f64) as usize;
            end = right.ceil().min(xmax as f64) as usize;
            values = collect(start, end);
        }
        if values.is_empty() {
            return Err(format!(
                "source color slice {} contained no foreground pixels",
                index + 1
            ));
        }
        let mut red = values.iter().map(|value| value[0]).collect::<Vec<_>>();
        let mut green = values.iter().map(|value| value[1]).collect::<Vec<_>>();
        let mut blue = values.iter().map(|value| value[2]).collect::<Vec<_>>();
        result.push([median(&mut red), median(&mut green), median(&mut blue)]);
    }
    Ok(result)
}

pub fn reduce(colors: &[Rgb], count: usize, method: Reduction) -> Vec<Rgb> {
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
            .map(|index| {
                let (start, end) = (index as f64 * scale, (index + 1) as f64 * scale);
                let (mut total, mut weight) = ([0.0; 3], 0.0);
                for (source_index, lab) in labs.iter().enumerate() {
                    let overlap = (end.min(source_index as f64 + 1.0)
                        - start.max(source_index as f64))
                    .max(0.0);
                    if overlap > 0.0 {
                        for channel in 0..3 {
                            total[channel] += lab[channel] * overlap;
                        }
                        weight += overlap;
                    }
                }
                oklab_to_rgb(total.map(|value| value / weight))
            })
            .collect();
    }
    (0..count)
        .map(|index| {
            let position = index as f64 * (labs.len() - 1) as f64 / (count - 1) as f64;
            let source_index = position.floor() as usize;
            let fraction = position - source_index as f64;
            if source_index >= labs.len() - 1 {
                oklab_to_rgb(labs[labs.len() - 1])
            } else {
                oklab_to_rgb(std::array::from_fn(|channel| {
                    labs[source_index][channel] * (1.0 - fraction)
                        + labs[source_index + 1][channel] * fraction
                }))
            }
        })
        .collect()
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
        assert_eq!(
            reduce(&colors, 2, Reduction::Resample),
            vec![[255, 0, 0], [0, 0, 255]]
        );
    }
}
