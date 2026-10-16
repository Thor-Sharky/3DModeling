//! Command-line flags. Most exist so the app can be driven and checked headlessly.

use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Args {
    /// Project to open.
    pub open: Option<PathBuf>,
    /// Save a screenshot of the whole window after `after_frames` frames, then quit.
    pub screenshot: Option<PathBuf>,
    pub after_frames: u32,
    /// Window size in logical pixels.
    pub size: Option<[f32; 2]>,
    /// Where preferences, autosaves and logs go (default: the OS data folder).
    pub data_dir: Option<PathBuf>,
    /// Multisampling for the viewport (0 = off). Default 4.
    pub msaa: u32,
    /// Print OpenGL information and quit.
    pub gl_info: bool,
    /// Render one frame offscreen, check it, and exit 0 (or 1).
    pub self_test_gl: bool,
    /// Start with a few sample shapes (used by screenshots and demos).
    pub demo: bool,
    /// Run a scenario script (see `script/`).
    pub script: Option<PathBuf>,
    /// Skip the "Save changes?" prompt when closing (scripts).
    pub no_save_prompt: bool,
    /// Override the autosave interval in seconds.
    pub autosave_secs: Option<u64>,
    /// Headless export: `--export PRESET IN OUT`.
    pub export: Option<(String, PathBuf, PathBuf)>,
    /// Headless picture: `--render IN --out PNG [--size WxH]`.
    pub render: Option<PathBuf>,
    pub out: Option<PathBuf>,
    /// Deterministic frames for screenshots: frozen clock and no blinking cursor.
    pub deterministic: bool,
}

pub const HELP: &str = "\
Shapeshop - easy 3D modeling

USAGE:
    shapeshop [FILE.shapes] [OPTIONS]

OPTIONS:
    --screenshot PNG        Save a screenshot after --after-frames frames, then quit
    --after-frames N        Frames to wait before the screenshot (default 10)
    --size WxH              Window size (default 1600x900)
    --data-dir DIR          Folder for preferences, autosaves and logs
    --msaa N                Viewport multisampling (0 = off, default 4)
    --demo                  Start with sample shapes
    --script FILE.ron       Run a scenario script
    --no-save-prompt        Don't ask to save when closing
    --autosave-secs N       Autosave interval in seconds
    --export PRESET IN OUT  Export a project without opening a window
    --render IN --out PNG   Save a picture of a project
    --gl-info               Print OpenGL information and quit
    --self-test-gl          Check that 3D drawing works, then quit
    -h, --help              Show this help
";

pub fn parse() -> Result<Args, String> {
    parse_from(std::env::args_os().skip(1))
}

pub fn parse_from(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Args, String> {
    use lexopt::prelude::*;
    let mut a = Args { after_frames: 10, msaa: 4, ..Default::default() };
    let mut p = lexopt::Parser::from_args(args);
    let e = |e: lexopt::Error| e.to_string();
    while let Some(arg) = p.next().map_err(e)? {
        match arg {
            Long("screenshot") => a.screenshot = Some(p.value().map_err(e)?.into()),
            Long("after-frames") => a.after_frames = p.value().map_err(e)?.parse().map_err(e)?,
            Long("size") => {
                let v = p.value().map_err(e)?.string().map_err(e)?;
                a.size = Some(parse_size(&v)?);
            }
            Long("data-dir") => a.data_dir = Some(p.value().map_err(e)?.into()),
            Long("msaa") => a.msaa = p.value().map_err(e)?.parse().map_err(e)?,
            Long("gl-info") => a.gl_info = true,
            Long("self-test-gl") => a.self_test_gl = true,
            Long("demo") => a.demo = true,
            Long("script") => a.script = Some(p.value().map_err(e)?.into()),
            Long("no-save-prompt") => a.no_save_prompt = true,
            Long("autosave-secs") => a.autosave_secs = Some(p.value().map_err(e)?.parse().map_err(e)?),
            Long("export") => {
                let preset = p.value().map_err(e)?.string().map_err(e)?;
                let input: PathBuf = p.value().map_err(e)?.into();
                let output: PathBuf = p.value().map_err(e)?.into();
                a.export = Some((preset, input, output));
            }
            Long("render") => a.render = Some(p.value().map_err(e)?.into()),
            Long("out") => a.out = Some(p.value().map_err(e)?.into()),
            Long("deterministic") => a.deterministic = true,
            Short('h') | Long("help") => return Err(HELP.to_owned()),
            Value(v) if a.open.is_none() => a.open = Some(v.into()),
            _ => return Err(arg.unexpected().to_string()),
        }
    }
    if a.screenshot.is_some() || a.script.is_some() {
        a.deterministic = true;
    }
    Ok(a)
}

fn parse_size(s: &str) -> Result<[f32; 2], String> {
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(|| format!("size must look like 1600x900, got {s}"))?;
    let w: f32 = w.trim().parse().map_err(|_| format!("bad width in {s}"))?;
    let h: f32 = h.trim().parse().map_err(|_| format!("bad height in {s}"))?;
    if w < 200.0 || h < 200.0 {
        return Err(format!("size {s} is too small"));
    }
    Ok([w, h])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Args, String> {
        parse_from(args.iter().map(std::ffi::OsString::from))
    }

    #[test]
    fn parses_flags() {
        let a =
            p(&["--screenshot", "out.png", "--after-frames", "5", "--size", "1280x720", "--msaa", "0", "my.shapes"])
                .unwrap();
        assert_eq!(a.screenshot.unwrap(), PathBuf::from("out.png"));
        assert_eq!(a.after_frames, 5);
        assert_eq!(a.size, Some([1280.0, 720.0]));
        assert_eq!(a.msaa, 0);
        assert_eq!(a.open.unwrap(), PathBuf::from("my.shapes"));
        assert!(a.deterministic);
        let a = p(&["--export", "roblox", "in.shapes", "out"]).unwrap();
        assert_eq!(a.export.unwrap().0, "roblox");
        assert!(p(&["--size", "10x10"]).is_err());
        assert!(p(&["--bogus"]).is_err());
    }
}
