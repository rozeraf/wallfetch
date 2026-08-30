#!/usr/bin/env python3
"""wallfetch: color a Fastfetch logo from the current Noctalia wallpaper.

Designed for wallpapers with several horizontally arranged color regions (such as
capsules / bands). It extracts N representative source colors from left to right,
reduces them perceptually to M Fastfetch logo colors, and launches fastfetch with
--logo-color-1 ... --logo-color-M true-color ANSI values.

Dependency: Pillow (Arch: python-pillow).
"""

from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys
from typing import Iterable, Sequence
from urllib.parse import unquote, urlparse

try:
    from PIL import Image, ImageOps
except ImportError:
    print(
        "wallfetch: Pillow is required. On Arch: sudo pacman -S python-pillow",
        file=sys.stderr,
    )
    raise SystemExit(2)


# ---------- color math (OKLab) ----------

def _srgb_to_linear(v: int) -> float:
    x = v / 255.0
    return x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4


def rgb_to_oklab(rgb: Sequence[int]) -> tuple[float, float, float]:
    r, g, b = (_srgb_to_linear(int(v)) for v in rgb)
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l_, m_, s_ = l ** (1 / 3), m ** (1 / 3), s ** (1 / 3)
    return (
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    )


def _linear_to_srgb(v: float) -> int:
    v = min(1.0, max(0.0, v))
    x = 12.92 * v if v <= 0.0031308 else 1.055 * (v ** (1 / 2.4)) - 0.055
    return int(round(min(1.0, max(0.0, x)) * 255.0))


def oklab_to_rgb(lab: Sequence[float]) -> tuple[int, int, int]:
    L, a, b = lab
    l_ = L + 0.3963377774 * a + 0.2158037573 * b
    m_ = L - 0.1055613458 * a - 0.0638541728 * b
    s_ = L - 0.0894841775 * a - 1.2914855480 * b
    l, m, s = l_**3, m_**3, s_**3
    r = +4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s
    g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s
    bb = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s
    return (_linear_to_srgb(r), _linear_to_srgb(g), _linear_to_srgb(bb))


def _lab_dist(a: Sequence[float], b: Sequence[float]) -> float:
    return math.sqrt(sum((a[i] - b[i]) ** 2 for i in range(3)))


def rgb_hex(rgb: Sequence[int]) -> str:
    return "#%02x%02x%02x" % tuple(int(v) for v in rgb)


def ansi_truecolor(rgb: Sequence[int]) -> str:
    r, g, b = (int(v) for v in rgb)
    return f"38;2;{r};{g};{b}"


# ---------- wallpaper discovery ----------

IMAGE_EXTS = {".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".tif", ".tiff", ".jxl", ".avif"}


def _normalize_path(text: str) -> Path | None:
    text = text.strip().strip("\"'")
    if text.startswith("file://"):
        text = unquote(urlparse(text).path)
    text = os.path.expandvars(os.path.expanduser(text))
    p = Path(text)
    try:
        if p.is_file():
            return p.resolve()
    except OSError:
        return None
    return None


def _existing_image_from_output(text: str) -> Path | None:
    # First try complete lines and the right-hand side of "connector: /path".
    for raw in reversed(text.splitlines()):
        line = raw.strip()
        for candidate in (line, line.split(": ", 1)[-1] if ": " in line else ""):
            p = _normalize_path(candidate)
            if p and (p.suffix.lower() in IMAGE_EXTS or p.is_file()):
                return p

    # Then try quoted substrings and absolute-path-looking fragments.
    candidates = re.findall(r'["\']([^"\']+)["\']', text)
    candidates += re.findall(r"(?:file://)?/[A-Za-z0-9_ .~+@%=/\\'()\[\],-]+", text)
    for candidate in reversed(candidates):
        p = _normalize_path(candidate)
        if p and (p.suffix.lower() in IMAGE_EXTS or p.is_file()):
            return p
    return None


def _run_for_path(cmd: list[str]) -> Path | None:
    try:
        proc = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=1.5,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if proc.stdout:
        return _existing_image_from_output(proc.stdout)
    return None


def _find_paths_in_obj(obj, context: str = "") -> list[Path]:
    found: list[Path] = []
    if isinstance(obj, dict):
        for k, v in obj.items():
            key_ctx = f"{context}.{k}" if context else str(k)
            found.extend(_find_paths_in_obj(v, key_ctx))
    elif isinstance(obj, list):
        for v in obj:
            found.extend(_find_paths_in_obj(v, context))
    elif isinstance(obj, str) and ("wallpaper" in context.lower() or "path" in context.lower()):
        p = _normalize_path(obj)
        if p and p.suffix.lower() in IMAGE_EXTS:
            found.append(p)
    return found


def _state_file_wallpaper() -> Path | None:
    # Noctalia v5 state first.
    state_home = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
    candidates = [
        state_home / "noctalia/state.toml",
        state_home / "noctalia/settings.toml",
    ]

    try:
        import tomllib  # Python 3.11+
    except ImportError:
        tomllib = None

    if tomllib:
        for f in candidates:
            try:
                if not f.is_file():
                    continue
                with f.open("rb") as fh:
                    obj = tomllib.load(fh)
                paths = _find_paths_in_obj(obj)
                if paths:
                    return paths[-1]
            except Exception:
                pass

    # Best-effort v4 JSON fallback.
    config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    for f in (config_home / "noctalia").glob("*.json") if (config_home / "noctalia").is_dir() else []:
        try:
            obj = json.loads(f.read_text(encoding="utf-8"))
            paths = _find_paths_in_obj(obj)
            if paths:
                return paths[-1]
        except Exception:
            pass
    return None


def detect_wallpaper(explicit: str | None, monitor: str | None) -> Path:
    if explicit:
        p = _normalize_path(explicit)
        if p:
            return p
        raise SystemExit(f"wallfetch: wallpaper does not exist: {explicit}")

    env_path = os.environ.get("NOCTALIA_WALLPAPER_PATH")
    if env_path:
        p = _normalize_path(env_path)
        if p:
            return p

    # Noctalia v5 native IPC.
    if shutil.which("noctalia"):
        cmd = ["noctalia", "msg", "wallpaper-get"]
        if monitor:
            cmd.append(monitor)
        p = _run_for_path(cmd)
        if p:
            return p

    # Noctalia v4 / Quickshell IPC.
    if shutil.which("qs"):
        cmd = ["qs", "ipc", "call", "wallpaper", "get"]
        if monitor:
            cmd.append(monitor)
        p = _run_for_path(cmd)
        if p:
            return p

    p = _state_file_wallpaper()
    if p:
        return p

    raise SystemExit(
        "wallfetch: could not determine the current Noctalia wallpaper.\n"
        "Pass it explicitly with --wallpaper /path/to/image or, on multi-monitor setups, --monitor DP-1."
    )


# ---------- extraction ----------

def _background_lab(im: Image.Image) -> tuple[float, float, float]:
    """Estimate the flat background from the most common coarse border color."""
    w, h = im.size
    px = im.load()
    bw = max(2, min(w, h) // 28)
    buckets: dict[tuple[int, int, int], int] = {}
    border: list[tuple[tuple[int, int, int], tuple[int, int, int]]] = []

    for y in range(h):
        for x in range(w):
            if x < bw or x >= w - bw or y < bw or y >= h - bw:
                rgb = px[x, y]
                key = (rgb[0] // 16, rgb[1] // 16, rgb[2] // 16)
                buckets[key] = buckets.get(key, 0) + 1
                border.append((rgb, key))

    mode_key = max(buckets, key=buckets.get)
    labs = [rgb_to_oklab(rgb) for rgb, key in border if key == mode_key]
    return tuple(sum(v[i] for v in labs) / len(labs) for i in range(3))


def _longest_run(flags: Sequence[bool]) -> tuple[int, int] | None:
    best: tuple[int, int] | None = None
    start: int | None = None
    for i, value in enumerate(list(flags) + [False]):
        if value and start is None:
            start = i
        elif not value and start is not None:
            end = i - 1
            if best is None or end - start > best[1] - best[0]:
                best = (start, end)
            start = None
    return best


def extract_horizontal_colors(
    wallpaper: Path,
    source_count: int = 8,
    sample_width: int = 224,
    sample_height: int = 112,
    background_threshold: float = 0.055,
    min_column_fraction: float = 0.08,
) -> list[tuple[int, int, int]]:
    """Extract left-to-right representative colors from a flat-background wallpaper.

    The method is intentionally spatial rather than a generic global palette: it
    finds the main foreground band, splits it into equal horizontal regions, and
    takes a robust median color from the middle of each region. This makes it
    suitable for capsule / stripe / block wallpapers where preserving the visual
    order matters more than returning globally dominant colors.
    """
    with Image.open(wallpaper) as raw:
        im = ImageOps.exif_transpose(raw).convert("RGB")
        im = im.resize((sample_width, sample_height), Image.Resampling.BILINEAR)

    w, h = im.size
    px = im.load()
    bg = _background_lab(im)
    masks: list[list[tuple[int, int, int]]] = [[] for _ in range(w)]

    for x in range(w):
        col = masks[x]
        for y in range(h):
            rgb = px[x, y]
            if _lab_dist(rgb_to_oklab(rgb), bg) > background_threshold:
                col.append(rgb)

    min_pixels = max(3, int(round(h * min_column_fraction)))
    run = _longest_run([len(col) >= min_pixels for col in masks])
    if run is None or run[1] - run[0] + 1 < source_count:
        raise RuntimeError(
            "could not find one sufficiently large horizontal foreground region; "
            "this extractor is tuned for flat-background capsule/band wallpapers"
        )

    xmin, xmax = run
    span = xmax - xmin + 1
    result: list[tuple[int, int, int]] = []

    for i in range(source_count):
        left = xmin + span * i / source_count
        right = xmin + span * (i + 1) / source_count
        mid = (left + right) / 2.0

        # Use the center 56% of each source slice. It avoids antialiased borders
        # and overlap seams while still leaving enough pixels for a stable median.
        half = (right - left) * 0.28
        xa = max(xmin, int(math.floor(mid - half)))
        xb = min(xmax, int(math.ceil(mid + half)))

        values: list[tuple[int, int, int]] = []
        for x in range(xa, xb + 1):
            values.extend(masks[x])

        if len(values) < 8:
            # Fallback to the whole slice if its center is unusually sparse.
            xa = max(xmin, int(math.floor(left)))
            xb = min(xmax, int(math.ceil(right)))
            values = []
            for x in range(xa, xb + 1):
                values.extend(masks[x])

        if not values:
            raise RuntimeError(f"source color slice {i + 1} contained no foreground pixels")

        r = int(round(statistics.median(v[0] for v in values)))
        g = int(round(statistics.median(v[1] for v in values)))
        b = int(round(statistics.median(v[2] for v in values)))
        result.append((r, g, b))

    return result


def reduce_colors(
    colors: Sequence[tuple[int, int, int]],
    output_count: int,
    method: str = "resample",
) -> list[tuple[int, int, int]]:
    if output_count <= 0:
        raise ValueError("output_count must be > 0")
    if not colors:
        raise ValueError("no colors to reduce")
    if output_count == 1:
        labs = [rgb_to_oklab(c) for c in colors]
        avg = tuple(sum(v[i] for v in labs) / len(labs) for i in range(3))
        return [oklab_to_rgb(avg)]

    labs = [rgb_to_oklab(c) for c in colors]
    n = len(labs)

    if method == "mean":
        # Area-preserving downsample: each output cell averages the source cells
        # that geometrically overlap it. This is the literal 8 -> 5 mean mode.
        scale = n / output_count
        out: list[tuple[int, int, int]] = []
        for j in range(output_count):
            a = j * scale
            b = (j + 1) * scale
            total = [0.0, 0.0, 0.0]
            weight = 0.0
            for i, lab in enumerate(labs):
                overlap = max(0.0, min(b, i + 1.0) - max(a, float(i)))
                if overlap:
                    for c in range(3):
                        total[c] += lab[c] * overlap
                    weight += overlap
            out.append(oklab_to_rgb(tuple(v / weight for v in total)))
        return out

    # Default: endpoint-preserving perceptual resample. The first and last
    # wallpaper colors stay exact; intermediate Fastfetch slots interpolate in
    # OKLab, which avoids muddy RGB averages.
    out = []
    for j in range(output_count):
        pos = j * (n - 1) / (output_count - 1)
        i = int(math.floor(pos))
        t = pos - i
        if i >= n - 1:
            lab = labs[-1]
        else:
            lab = tuple(labs[i][c] * (1.0 - t) + labs[i + 1][c] * t for c in range(3))
        out.append(oklab_to_rgb(lab))
    return out


# ---------- cache ----------

def cache_path() -> Path:
    base = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    return base / "noctalia-fastfetch/palette.json"


def load_cache(wallpaper: Path, args) -> tuple[list[tuple[int, int, int]], list[tuple[int, int, int]]] | None:
    f = cache_path()
    try:
        obj = json.loads(f.read_text(encoding="utf-8"))
        st = wallpaper.stat()
        key = {
            "wallpaper": str(wallpaper),
            "mtime_ns": st.st_mtime_ns,
            "size": st.st_size,
            "source_count": args.source_count,
            "logo_count": args.logo_count,
            "reduction": args.reduction,
            "background_threshold": args.background_threshold,
        }
        if obj.get("key") != key:
            return None
        source = [tuple(v) for v in obj["source_colors"]]
        logo = [tuple(v) for v in obj["logo_colors"]]
        return source, logo
    except Exception:
        return None


def save_cache(wallpaper: Path, source, logo, args) -> None:
    f = cache_path()
    f.parent.mkdir(parents=True, exist_ok=True)
    st = wallpaper.stat()
    obj = {
        "key": {
            "wallpaper": str(wallpaper),
            "mtime_ns": st.st_mtime_ns,
            "size": st.st_size,
            "source_count": args.source_count,
            "logo_count": args.logo_count,
            "reduction": args.reduction,
            "background_threshold": args.background_threshold,
        },
        "source_colors": [list(v) for v in source],
        "logo_colors": [list(v) for v in logo],
        "source_hex": [rgb_hex(v) for v in source],
        "logo_hex": [rgb_hex(v) for v in logo],
    }
    tmp = f.with_suffix(".tmp")
    tmp.write_text(json.dumps(obj, indent=2) + "\n", encoding="utf-8")
    tmp.replace(f)


# ---------- CLI ----------

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="wallfetch",
        description="Extract ordered wallpaper colors and apply them to Fastfetch logo color slots.",
        add_help=True,
    )
    p.add_argument("--wallpaper", help="explicit wallpaper path; otherwise auto-detect Noctalia")
    p.add_argument("--monitor", help="Noctalia output connector, e.g. DP-1 (useful on multi-monitor setups)")
    p.add_argument("--source-count", type=int, default=8, help="source color regions to extract (default: 8)")
    p.add_argument("--logo-count", type=int, default=5, choices=range(1, 10), metavar="1..9", help="Fastfetch logo color slots (default: 5)")
    p.add_argument("--reduction", choices=("resample", "mean"), default="resample", help="8->5 reduction: endpoint-preserving resample (default) or literal area mean")
    p.add_argument("--background-threshold", type=float, default=0.055, help="OKLab distance from border background (default: 0.055)")
    p.add_argument("--print", dest="print_colors", action="store_true", help="print detected colors and exit")
    p.add_argument("--json", dest="json_output", action="store_true", help="print colors as JSON and exit")
    p.add_argument("--cache-only", action="store_true", help="refresh cache and exit (useful from a Noctalia hook)")
    p.add_argument("--no-cache", action="store_true", help="force re-extraction")
    return p


def main() -> int:
    parser = build_parser()
    args, fastfetch_args = parser.parse_known_args()

    if args.source_count < 1:
        parser.error("--source-count must be >= 1")

    wallpaper = detect_wallpaper(args.wallpaper, args.monitor)

    cached = None if args.no_cache else load_cache(wallpaper, args)
    if cached:
        source_colors, logo_colors = cached
    else:
        try:
            source_colors = extract_horizontal_colors(
                wallpaper,
                source_count=args.source_count,
                background_threshold=args.background_threshold,
            )
            logo_colors = reduce_colors(source_colors, args.logo_count, args.reduction)
        except Exception as e:
            print(f"wallfetch: color extraction failed: {e}", file=sys.stderr)
            return 2
        save_cache(wallpaper, source_colors, logo_colors, args)

    if args.json_output:
        print(json.dumps({
            "wallpaper": str(wallpaper),
            "source": [rgb_hex(c) for c in source_colors],
            "logo": [rgb_hex(c) for c in logo_colors],
            "fastfetch": {str(i + 1): ansi_truecolor(c) for i, c in enumerate(logo_colors)},
        }, indent=2))
        return 0

    if args.print_colors:
        print(f"wallpaper: {wallpaper}")
        print("source: " + "  ".join(rgb_hex(c) for c in source_colors))
        print("logo:   " + "  ".join(rgb_hex(c) for c in logo_colors))
        print("fastfetch args:")
        print("  " + " ".join(
            f"--logo-color-{i + 1} '{ansi_truecolor(c)}'" for i, c in enumerate(logo_colors)
        ))
        return 0

    if args.cache_only:
        return 0

    if not shutil.which("fastfetch"):
        print("wallfetch: fastfetch was not found in PATH", file=sys.stderr)
        return 127

    cmd = ["fastfetch"]
    for i, color in enumerate(logo_colors, start=1):
        cmd.extend([f"--logo-color-{i}", ansi_truecolor(color)])
    cmd.extend(fastfetch_args)
    os.execvp(cmd[0], cmd)
    return 127


if __name__ == "__main__":
    raise SystemExit(main())
