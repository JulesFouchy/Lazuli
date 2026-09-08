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
        // A truncated mp4 that looks like a finished export is worse than none.
        let _ = std::fs::remove_file(&self.output);
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
        let err = Encode::start(Path::new("ffmpeg"), &options)
            .expect_err("a zero hold should be rejected");
        assert!(err.to_string().contains("positive"));
    }

    #[test]
    fn a_missing_resource_dir_falls_through_to_path() {
        // Should not panic on a directory that does not exist; the result
        // depends on whether the machine has ffmpeg, so only the call is
        // asserted here.
        let _ = find_ffmpeg(Some(Path::new("C:/definitely/not/here")));
    }
}
