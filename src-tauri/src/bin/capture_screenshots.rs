use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, DynamicImage, Frame, RgbaImage};
use std::env;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use win_screenshot::prelude::*;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = match parse_args(env::args().skip(1)) {
        Ok(config) => config,
        Err(err) if err == "help requested" => return Ok(()),
        Err(err) => return Err(err),
    };
    let title = config.title;
    let out_dir = config.out_dir;
    let delay_ms = config.delay_ms;
    let initial_wait_ms = config.initial_wait_ms;

    if initial_wait_ms > 0 {
        sleep(Duration::from_millis(initial_wait_ms));
    }

    let hwnd = find_window_handle(&title)?;
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let context_path = out_dir.join("context-menu.png");
    let guidelines_path = out_dir.join("guidelines.png");
    let watch_path = out_dir.join("watch-mode.gif");

    capture_png(hwnd, &context_path)?;
    sleep(Duration::from_millis(delay_ms));

    capture_png(hwnd, &guidelines_path)?;
    sleep(Duration::from_millis(delay_ms));

    capture_gif(hwnd, &watch_path, delay_ms)?;

    println!("saved: {}", context_path.display());
    println!("saved: {}", guidelines_path.display());
    println!("saved: {}", watch_path.display());
    Ok(())
}

fn print_help() {
    println!(
        "Usage: capture_screenshots [--title <window_title>] [--out-dir <path>] [--delay-ms <ms>] [--wait-ms <ms>]\n\
Defaults:\n\
  --title shoruichecker\n\
  --out-dir docs/screenshots\n\
  --delay-ms 1200\n\
  --wait-ms 0"
    );
}

#[derive(Debug, PartialEq)]
struct Config {
    title: String,
    out_dir: PathBuf,
    delay_ms: u64,
    initial_wait_ms: u64,
}

fn parse_args<I>(mut args: I) -> Result<Config, String>
where
    I: Iterator<Item = String>,
{
    let mut config = Config {
        title: "shoruichecker".to_string(),
        out_dir: PathBuf::from("docs/screenshots"),
        delay_ms: 1200,
        initial_wait_ms: 0,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--title" => {
                config.title = args.next().ok_or("missing value for --title")?;
            }
            "--out-dir" => {
                config.out_dir = PathBuf::from(args.next().ok_or("missing value for --out-dir")?);
            }
            "--delay-ms" => {
                config.delay_ms = args
                    .next()
                    .ok_or("missing value for --delay-ms")?
                    .parse()
                    .map_err(|_| "invalid number for --delay-ms")?;
            }
            "--wait-ms" => {
                config.initial_wait_ms = args
                    .next()
                    .ok_or("missing value for --wait-ms")?
                    .parse()
                    .map_err(|_| "invalid number for --wait-ms")?;
            }
            "--help" | "-h" => {
                print_help();
                return Err("help requested".to_string());
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }

    Ok(config)
}

fn find_window_handle(title: &str) -> Result<isize, String> {
    if let Ok(hwnd) = find_window(title) {
        return Ok(hwnd);
    }

    let title_lc = title.to_lowercase();
    let windows = window_list().map_err(|e| format!("{e:?}"))?;
    for win in windows {
        if win
            .window_name
            .to_lowercase()
            .contains(title_lc.as_str())
        {
            return Ok(win.hwnd);
        }
    }

    Err(format!("window not found: {title}"))
}

fn capture_png(hwnd: isize, path: &Path) -> Result<(), String> {
    let image = capture_window_image(hwnd)?;
    image
        .to_rgb8()
        .save(path)
        .map_err(|e| e.to_string())
}

fn capture_gif(hwnd: isize, path: &Path, delay_ms: u64) -> Result<(), String> {
    let mut frames = Vec::with_capacity(2);
    frames.push(capture_window_image(hwnd)?.to_rgba8());
    sleep(Duration::from_millis(delay_ms));
    frames.push(capture_window_image(hwnd)?.to_rgba8());

    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = GifEncoder::new(file);
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|e| e.to_string())?;

    let delay = Delay::from_numer_denom_ms(delay_ms as u32, 1);
    for frame in frames {
        let gif_frame = Frame::from_parts(frame, 0, 0, delay);
        encoder.encode_frame(gif_frame).map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn capture_window_image(hwnd: isize) -> Result<DynamicImage, String> {
    let buffer = capture_window(hwnd).map_err(|e| e.to_string())?;
    let image = RgbaImage::from_raw(buffer.width, buffer.height, buffer.pixels)
        .ok_or("failed to build image buffer")?;
    Ok(DynamicImage::ImageRgba8(image))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults() {
        let config = parse_args(std::iter::empty()).expect("defaults");
        assert_eq!(config.title, "shoruichecker");
        assert_eq!(config.out_dir, PathBuf::from("docs/screenshots"));
        assert_eq!(config.delay_ms, 1200);
        assert_eq!(config.initial_wait_ms, 0);
    }

    #[test]
    fn parse_args_overrides() {
        let args = vec![
            "--title".to_string(),
            "Demo".to_string(),
            "--out-dir".to_string(),
            "out".to_string(),
            "--delay-ms".to_string(),
            "2500".to_string(),
            "--wait-ms".to_string(),
            "900".to_string(),
        ];
        let config = parse_args(args.into_iter()).expect("overrides");
        assert_eq!(config.title, "Demo");
        assert_eq!(config.out_dir, PathBuf::from("out"));
        assert_eq!(config.delay_ms, 2500);
        assert_eq!(config.initial_wait_ms, 900);
    }
}
