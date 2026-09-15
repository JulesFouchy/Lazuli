//! Video export: piping rendered frames into ffmpeg.
//!
//! Frames never touch disk. At a thousand-plus entries a temp directory of
//! 1080p PNGs would run to tens of gigabytes, so the frontend renders one frame
//! at a time and each is written straight to ffmpeg's stdin. ffmpeg's own
//! backpressure paces the loop, which keeps memory flat regardless of how many
//! entries a project has.

use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Keep ffmpeg from flashing a console window on Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How ffmpeg was found, so the UI can explain a missing encoder precisely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegSource {
    /// Shipped with the app.
    Bundled,
    /// Found on PATH.
    System,
}

/// An export in progress.
pub struct Encode {
    child: Child,
    stdin: Option<ChildStdin>,
    output: PathBuf,
    pub frames_written: u32,
}

/// Settings for one export.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExportOptions {
    pub output: PathBuf,
    /// How long each entry is held on screen.
    pub seconds_per_frame: f64,
    pub width: u32,
    pub height: u32,
    /// Output frame rate. Frames are held for `seconds_per_frame`, but the file
    /// is written at this rate so it plays smoothly everywhere.
    pub fps: u32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            output: PathBuf::from("summary.mp4"),
            seconds_per_frame: 1.0,
            width: 1920,
            height: 1080,
            fps: 30,
        }
    }
}

/// Locate ffmpeg: the copy shipped beside the app first, then PATH.
///
/// `resource_dir` is where Tauri puts bundled resources; in a dev build that is
/// the crate folder, so a binary dropped in `src-tauri/binaries` is picked up
/// without a rebuild.
pub fn find_ffmpeg(resource_dir: Option<&Path>) -> Option<(PathBuf, FfmpegSource)> {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

    if let Some(dir) = resource_dir {
        for candidate in [dir.join(name), dir.join("binaries").join(name)] {
            if candidate.is_file() {
                return Some((candidate, FfmpegSource::Bundled));
            }
        }
    }

    // `--version` is the cheapest way to ask "is this runnable?".
    let mut probe = Command::new(name);
    probe.arg("-version").stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    probe.creation_flags(CREATE_NO_WINDOW);

    match probe.status() {
        Ok(status) if status.success() => Some((PathBuf::from(name), FfmpegSource::System)),
        _ => None,
    }
}

impl Encode {
    /// Start ffmpeg and leave it waiting for frames on stdin.
    pub fn start(ffmpeg: &Path, options: &ExportOptions) -> Result<Self> {
        if let Some(parent) = options.output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
        }
        if options.seconds_per_frame <= 0.0 {
            bail!("each frame must be held for a positive number of seconds");
        }

        // The input rate is one frame per `seconds_per_frame`; `-r` then
        // resamples to a normal playback rate so players do not stutter.
        let input_rate = format!("{:.6}", 1.0 / options.seconds_per_frame);

        let mut command = Command::new(ffmpeg);
        command
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "image2pipe", "-vcodec", "png"])
            .args(["-framerate", &input_rate])
            .args(["-i", "-"])
            .args(["-r", &options.fps.to_string()])
            .args(["-c:v", "libx264", "-preset", "medium", "-crf", "20"])
            // yuv420p is what makes the file playable outside ffmpeg itself.
            .args(["-pix_fmt", "yuv420p"])
            // H.264 needs even dimensions; guard against an odd export size
            // rather than failing several hundred frames in.
            .args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"])
            .args(["-movflags", "+faststart"])
            .arg(&options.output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command
            .spawn()
            .with_context(|| format!("starting {}", ffmpeg.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("ffmpeg gave us no stdin to write frames to"))?;

        Ok(Self {
            child,
            stdin: Some(stdin),
            output: options.output.clone(),
            frames_written: 0,
        })
    }

    /// Hand one PNG-encoded frame to ffmpeg.
    pub fn push_frame(&mut self, png: &[u8]) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("this export has already been finished"))?;

        // A broken pipe means ffmpeg died; its stderr says why, and reporting
        // that is far more useful than "failed to write to stdin".
        if let Err(err) = stdin.write_all(png) {
            let reason = self.take_stderr();
            bail!("ffmpeg stopped accepting frames ({err}){reason}");
        }
        self.frames_written += 1;
        Ok(())
    }

    /// Close stdin and wait for ffmpeg to finish writing the file.
    pub fn finish(mut self) -> Result<PathBuf> {
        // Dropping stdin is what tells ffmpeg the stream is over.
        self.stdin.take();

        let status = self.child.wait().context("waiting for ffmpeg to finish")?;
        if !status.success() {
            let reason = self.take_stderr();
            bail!("ffmpeg exited with {status}{reason}");
        }
        if self.frames_written == 0 {
            bail!("no frames were rendered, so there is nothing to export");
        }
        Ok(self.output)
    }

    /// Stop the encode and remove the half-written file.
    pub fn cancel(mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();

        // A truncated mp4 that looks like a finished export is worse than no
        // file at all, so this is worth retrying: on Windows the output handle
        // can briefly outlive the terminated process, and the first delete
        // then fails with "in use by another process".
        for attempt in 0..20 {
            if !self.output.exists() {
                return;
            }
            if std::fs::remove_file(&self.output).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25 * (attempt + 1)));
        }
        eprintln!(
            "lapis: could not remove the cancelled export at {}",
            self.output.display()
        );
    }

    /// Whatever ffmpeg complained about, formatted for appending to an error.
    fn take_stderr(&mut self) -> String {
        use std::io::Read;
        let Some(mut stderr) = self.child.stderr.take() else {
            return String::new();
        };
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        let text = text.trim();
        if text.is_empty() {
            String::new()
        } else {
            format!(": {text}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_length_frame_hold_is_refused() {
        // Would divide by zero when computing the input frame rate.
        let options = ExportOptions {
            seconds_per_frame: 0.0,
            ..Default::default()
        };
        // `expect_err` would need `Encode: Debug`, which would mean deriving
        // it on a struct holding a live child process.
        let Err(err) = Encode::start(Path::new("ffmpeg"), &options) else {
            panic!("a zero-length frame hold should be rejected");
        };
        assert!(err.to_string().contains("positive"));
    }

    /// A minimal valid PNG of a solid colour, so the pipe can be tested
    /// without pulling in an image crate.
    // `same_item_push` fires on the per-row filter byte below and is wrong
    // about it: the zero is not a repeated fill but one byte interleaved with
    // each row's pixels, which is the PNG scanline format. Collapsing it into a
    // `vec![0; n]` the way the lint suggests would produce a corrupt image.
    #[allow(clippy::same_item_push)]
    fn solid_png(width: u32, height: u32, shade: u8) -> Vec<u8> {
        use std::io::Write as _;

        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for byte in bytes {
                crc ^= *byte as u32;
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xEDB8_8320 & (!(crc & 1)).wrapping_add(1));
                }
            }
            !crc
        }

        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
            out
        }

        // Uncompressed deflate blocks: no compressor needed.
        fn store(raw: &[u8]) -> Vec<u8> {
            let mut out = vec![0x78, 0x01];
            for (index, block) in raw.chunks(65_535).enumerate() {
                let last = (index + 1) * 65_535 >= raw.len();
                out.push(if last { 1 } else { 0 });
                out.extend_from_slice(&(block.len() as u16).to_le_bytes());
                out.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
                out.extend_from_slice(block);
            }
            let (mut a, mut b) = (1u32, 0u32);
            for byte in raw {
                a = (a + *byte as u32) % 65521;
                b = (b + a) % 65521;
            }
            let _ = out.write_all(&((b << 16) | a).to_be_bytes());
            out
        }

        let mut raw = Vec::new();
        for _ in 0..height {
            raw.push(0); // filter: none
            for _ in 0..width {
                raw.extend_from_slice(&[shade, shade / 2, 255 - shade]);
            }
        }

        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);

        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"IDAT", &store(&raw)));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    /// The real thing: spawn ffmpeg, stream frames through stdin, get a file.
    ///
    /// Skipped when the machine has no ffmpeg, since that is an environment
    /// gap rather than a code failure.
    #[test]
    fn frames_piped_to_ffmpeg_produce_a_playable_file() {
        let Some((ffmpeg, _)) = find_ffmpeg(None) else {
            eprintln!("skipping: no ffmpeg on PATH");
            return;
        };

        let output = std::env::temp_dir()
            .join(format!("lapis-export-{}.mp4", uuid::Uuid::new_v4()));
        let options = ExportOptions {
            output: output.clone(),
            seconds_per_frame: 0.5,
            width: 320,
            height: 240,
            fps: 24,
        };

        let mut encode = Encode::start(&ffmpeg, &options).expect("ffmpeg should start");
        for frame in 0..6u8 {
            encode
                .push_frame(&solid_png(320, 240, frame * 40))
                .expect("ffmpeg should accept the frame");
        }
        assert_eq!(encode.frames_written, 6);

        let written = encode.finish().expect("ffmpeg should finish cleanly");
        let size = std::fs::metadata(&written)
            .expect("the output file should exist")
            .len();
        assert!(size > 0, "the exported file should not be empty");
        let _ = std::fs::remove_file(&written);
    }

    #[test]
    fn an_export_with_no_frames_is_an_error_not_an_empty_file() {
        let Some((ffmpeg, _)) = find_ffmpeg(None) else {
            eprintln!("skipping: no ffmpeg on PATH");
            return;
        };
        let output = std::env::temp_dir()
            .join(format!("lapis-empty-{}.mp4", uuid::Uuid::new_v4()));
        let encode = Encode::start(
            &ffmpeg,
            &ExportOptions {
                output: output.clone(),
                ..Default::default()
            },
        )
        .expect("ffmpeg should start");
        // ffmpeg itself fails on an empty stream; either way this must not
        // hand back a path to a file that does not play.
        assert!(encode.finish().is_err());
        let _ = std::fs::remove_file(&output);
    }

    #[test]
    fn cancelling_removes_the_half_written_file() {
        let Some((ffmpeg, _)) = find_ffmpeg(None) else {
            eprintln!("skipping: no ffmpeg on PATH");
            return;
        };
        let output = std::env::temp_dir()
            .join(format!("lapis-cancel-{}.mp4", uuid::Uuid::new_v4()));
        let mut encode = Encode::start(
            &ffmpeg,
            &ExportOptions {
                output: output.clone(),
                width: 320,
                height: 240,
                ..Default::default()
            },
        )
        .expect("ffmpeg should start");
        encode.push_frame(&solid_png(320, 240, 90)).expect("frame accepted");
        encode.cancel();
        // A truncated mp4 left behind would look like a finished export.
        assert!(!output.exists(), "cancel should leave no file behind");
    }

    #[test]
    fn a_missing_resource_dir_falls_through_to_path() {
        // Should not panic on a directory that does not exist; the result
        // depends on whether the machine has ffmpeg, so only the call is
        // asserted here.
        let _ = find_ffmpeg(Some(Path::new("C:/definitely/not/here")));
    }
}
