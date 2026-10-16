//! Scenario scripts: drive the real app with injected mouse/keyboard input,
//! take screenshots and check results. Used for headless verification.
//!
//! ```ron
//! Script(steps: [
//!     Click(Anchor("shapes.block")),
//!     Assert(ObjectCount(1)),
//!     Drag(from: World(ObjectCenter("Block 1")), to: Offset(120, 0)),
//!     Screenshot(path: "added.png", probes: []),
//! ])
//! ```
//!
//! Each frame advances time by a fixed 1/60 s, so runs are deterministic
//! regardless of how slow software rendering is.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::app::ShapeshopApp;

pub const FRAME_DT: f64 = 1.0 / 60.0;
const STEP_TIMEOUT_FRAMES: u32 = 900;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Script {
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Btn {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mods {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WorldTarget {
    Point(f32, f32, f32),
    ObjectCenter(String),
    /// Center of a face of a named shape (face ids are fixed per primitive: Block 2 = top).
    FaceCenter(String, u32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Target {
    /// A named UI element (see `anchors.rs`).
    Anchor(String),
    /// Absolute window position.
    Pos(f32, f32),
    /// A point in the 3D scene, projected to the screen.
    World(WorldTarget),
    /// Relative to the last pointer position.
    Offset(f32, f32),
    ViewportCenter,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Check {
    ObjectCount(usize),
    SelectedCount(usize),
    SelectedName(String),
    ObjectPosition {
        name: String,
        position: (f32, f32, f32),
        tolerance: f32,
    },
    ObjectSize {
        name: String,
        size: (f32, f32, f32),
        tolerance: f32,
    },
    HistoryLen(usize),
    CanUndo(bool),
    Dirty(bool),
    ToolIs(String),
    /// The document equals the one remembered with `Remember(tag)`.
    SameAs(String),
    AnchorExists(String),
    FileExists(String),
    StatusContains(String),
    /// Controls preset key: "roblox", "trackpad", "blender".
    Controls(String),
    /// Project units: "studs" or "m".
    Units(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Probe {
    pub at: Target,
    pub rgb: (u8, u8, u8),
    #[serde(default = "default_tolerance")]
    pub tolerance: u8,
    /// Expect the color NOT to match.
    #[serde(default)]
    pub not: bool,
}

fn default_tolerance() -> u8 {
    40
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Step {
    WaitFrames(u32),
    /// Let previews and animations settle (a few quiet frames).
    WaitIdle,
    Click(Target),
    ClickWith {
        at: Target,
        button: Btn,
        #[serde(default)]
        mods: Mods,
    },
    DoubleClick(Target),
    Hover(Target),
    Drag {
        from: Target,
        to: Target,
        #[serde(default)]
        button: Option<Btn>,
        #[serde(default)]
        frames: Option<u32>,
        #[serde(default)]
        mods: Mods,
    },
    Scroll {
        at: Target,
        delta: f32,
    },
    Key(String),
    KeyWith {
        key: String,
        mods: Mods,
    },
    Type(String),
    Action(ss_ops::AppAction),
    Assert(Check),
    /// Save the current document under a name, for `Assert(SameAs(..))`.
    Remember(String),
    Screenshot {
        path: String,
        #[serde(default)]
        probes: Vec<Probe>,
    },
    Log(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct StepReport {
    pub index: usize,
    pub step: String,
    pub ok: bool,
    pub message: String,
    pub frame: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub script: String,
    pub passed: bool,
    pub steps: Vec<StepReport>,
    pub screenshots: Vec<String>,
}

enum Phase {
    Start,
    /// Waiting for queued input to be consumed, then `frames` more frames.
    Settling(u32),
    WaitingScreenshot {
        path: PathBuf,
        probes: Vec<Probe>,
    },
}

pub struct ScriptRunner {
    pub name: String,
    steps: Vec<Step>,
    index: usize,
    phase: Phase,
    queue: VecDeque<Vec<Event>>,
    pointer: Pos2,
    step_frames: u32,
    frame: u64,
    pub out_dir: PathBuf,
    remembered: std::collections::HashMap<String, ss_doc::Document>,
    report: Report,
    pub finished: Option<bool>,
}

impl ScriptRunner {
    pub fn load(path: &Path, out_dir: PathBuf) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let script: Script = ron::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Self::new(name, script, out_dir))
    }

    pub fn new(name: String, script: Script, out_dir: PathBuf) -> Self {
        Self {
            report: Report { script: name.clone(), passed: false, steps: vec![], screenshots: vec![] },
            name,
            steps: script.steps,
            index: 0,
            phase: Phase::Start,
            queue: VecDeque::new(),
            pointer: Pos2::new(800.0, 450.0),
            step_frames: 0,
            frame: 0,
            out_dir,
            remembered: Default::default(),
            finished: None,
        }
    }

    /// Called from `App::raw_input_hook`: fixed time step + this frame's injected events.
    pub fn inject(&mut self, raw: &mut egui::RawInput) {
        self.frame += 1;
        raw.time = Some(self.frame as f64 * FRAME_DT);
        raw.predicted_dt = FRAME_DT as f32;
        // Real input is ignored while a script runs (deterministic).
        raw.events.retain(|e| matches!(e, Event::Screenshot { .. } | Event::WindowFocused(_)));
        if let Some(events) = self.queue.pop_front() {
            raw.events.extend(events);
        }
    }

    fn resolve(&self, app: &mut ShapeshopApp, t: &Target) -> Result<Pos2, String> {
        match t {
            Target::Pos(x, y) => Ok(Pos2::new(*x, *y)),
            Target::Offset(dx, dy) => Ok(self.pointer + egui::vec2(*dx, *dy)),
            Target::Anchor(id) => {
                app.anchors.get(id).map(|r| r.center()).ok_or_else(|| format!("no UI element named {id:?}"))
            }
            Target::ViewportCenter => app.viewport_rect.map(|r| r.center()).ok_or_else(|| "no viewport yet".into()),
            Target::World(w) => {
                let p = match w {
                    WorldTarget::Point(x, y, z) => Vec3::new(*x, *y, *z),
                    WorldTarget::ObjectCenter(name) => {
                        let id = find(app, name)?;
                        app.doc.world_bounds(id).center()
                    }
                    WorldTarget::FaceCenter(name, face) => {
                        let id = find(app, name)?;
                        let mesh = app.eval.shape_mesh(&app.doc, id).ok_or("not a shape")?;
                        let c = mesh
                            .face_center(ss_mesh::FaceId(*face))
                            .ok_or_else(|| format!("{name} has no face {face}"))?;
                        app.doc.world_transform(id).transform_point(c)
                    }
                };
                let rect = app.viewport_rect.ok_or("no viewport yet")?;
                let vp = crate::viewport::egui_to_viewport(rect);
                let s = app.camera.project(&vp, p).ok_or("point is behind the camera")?;
                Ok(Pos2::new(s.x, s.y))
            }
        }
    }

    fn fail(&mut self, msg: String) {
        log::error!("script {} step {} failed: {msg}", self.name, self.index);
        self.record(false, msg);
        self.finished = Some(false);
    }

    fn record(&mut self, ok: bool, message: String) {
        let step = self.steps.get(self.index).map(|s| format!("{s:?}")).unwrap_or_default();
        self.report.steps.push(StepReport { index: self.index, step, ok, message, frame: self.frame });
    }

    fn next_step(&mut self, msg: impl Into<String>) {
        self.record(true, msg.into());
        self.index += 1;
        self.phase = Phase::Start;
        self.step_frames = 0;
    }

    fn mods(m: &Mods) -> Modifiers {
        Modifiers { alt: m.alt, ctrl: m.ctrl, shift: m.shift, mac_cmd: false, command: m.ctrl }
    }

    fn button(b: &Btn) -> PointerButton {
        match b {
            Btn::Left => PointerButton::Primary,
            Btn::Right => PointerButton::Secondary,
            Btn::Middle => PointerButton::Middle,
        }
    }

    fn queue_click(&mut self, pos: Pos2, button: PointerButton, mods: Modifiers, double: bool) {
        self.queue.push_back(vec![Event::PointerMoved(pos)]);
        self.queue.push_back(vec![Event::PointerMoved(pos)]);
        for _ in 0..if double { 2 } else { 1 } {
            self.queue.push_back(vec![Event::PointerButton { pos, button, pressed: true, modifiers: mods }]);
            self.queue.push_back(vec![Event::PointerButton { pos, button, pressed: false, modifiers: mods }]);
        }
        self.pointer = pos;
    }

    fn queue_key(&mut self, name: &str, mods: Modifiers) -> Result<(), String> {
        let key = Key::from_name(name).ok_or_else(|| format!("unknown key {name:?}"))?;
        if mods != Modifiers::NONE {
            self.queue.push_back(vec![Event::ModifiersChanged(mods)]);
        }
        self.queue.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: mods,
        }]);
        self.queue.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers: mods,
        }]);
        if mods != Modifiers::NONE {
            self.queue.push_back(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        }
        Ok(())
    }

    /// Called once per frame after the UI was laid out.
    pub fn tick(&mut self, app: &mut ShapeshopApp, ctx: &egui::Context) {
        if self.finished.is_some() {
            return;
        }
        ctx.request_repaint();
        self.step_frames += 1;
        if self.step_frames > STEP_TIMEOUT_FRAMES {
            self.fail("timed out".into());
            return;
        }
        let Some(step) = self.steps.get(self.index).cloned() else {
            self.report.passed = true;
            self.finished = Some(true);
            return;
        };
        match std::mem::replace(&mut self.phase, Phase::Start) {
            Phase::Settling(n) => {
                if !self.queue.is_empty() {
                    self.phase = Phase::Settling(n);
                } else if n > 0 {
                    self.phase = Phase::Settling(n - 1);
                } else {
                    self.next_step("ok");
                }
                return;
            }
            Phase::WaitingScreenshot { path, probes } => {
                if app.screenshots.is_busy() {
                    self.phase = Phase::WaitingScreenshot { path, probes };
                    return;
                }
                match check_probes(self, app, &path, &probes) {
                    Ok(()) => {
                        self.report.screenshots.push(path.display().to_string());
                        self.next_step(format!("saved {}", path.display()));
                    }
                    Err(e) => self.fail(e),
                }
                return;
            }
            Phase::Start => {}
        }
        let result: Result<u32, String> = (|| match &step {
            Step::WaitFrames(n) => Ok(*n),
            Step::WaitIdle => Ok(4),
            Step::Click(t) => {
                let p = self.resolve(app, t)?;
                self.queue_click(p, PointerButton::Primary, Modifiers::NONE, false);
                Ok(2)
            }
            Step::ClickWith { at, button, mods } => {
                let p = self.resolve(app, at)?;
                self.queue_click(p, Self::button(button), Self::mods(mods), false);
                Ok(2)
            }
            Step::DoubleClick(t) => {
                let p = self.resolve(app, t)?;
                self.queue_click(p, PointerButton::Primary, Modifiers::NONE, true);
                Ok(2)
            }
            Step::Hover(t) => {
                let p = self.resolve(app, t)?;
                self.queue.push_back(vec![Event::PointerMoved(p)]);
                self.pointer = p;
                Ok(2)
            }
            Step::Drag { from, to, button, frames, mods } => {
                let a = self.resolve(app, from)?;
                self.pointer = a;
                let b = self.resolve(app, to)?;
                let button = button.as_ref().map(Self::button).unwrap_or(PointerButton::Primary);
                let modifiers = Self::mods(mods);
                let n = frames.unwrap_or(12).max(2);
                if modifiers != Modifiers::NONE {
                    self.queue.push_back(vec![Event::ModifiersChanged(modifiers)]);
                }
                self.queue.push_back(vec![Event::PointerMoved(a)]);
                self.queue.push_back(vec![Event::PointerMoved(a)]);
                self.queue.push_back(vec![Event::PointerButton { pos: a, button, pressed: true, modifiers }]);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    self.queue.push_back(vec![Event::PointerMoved(a + (b - a) * t)]);
                }
                self.queue.push_back(vec![Event::PointerMoved(b)]);
                self.queue.push_back(vec![Event::PointerButton { pos: b, button, pressed: false, modifiers }]);
                if modifiers != Modifiers::NONE {
                    self.queue.push_back(vec![Event::ModifiersChanged(Modifiers::NONE)]);
                }
                self.pointer = b;
                Ok(3)
            }
            Step::Scroll { at, delta } => {
                let p = self.resolve(app, at)?;
                self.queue.push_back(vec![Event::PointerMoved(p)]);
                self.queue.push_back(vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, *delta),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                }]);
                self.pointer = p;
                Ok(8)
            }
            Step::Key(k) => {
                self.queue_key(k, Modifiers::NONE)?;
                Ok(2)
            }
            Step::KeyWith { key, mods } => {
                self.queue_key(key, Self::mods(mods))?;
                Ok(2)
            }
            Step::Type(text) => {
                self.queue.push_back(vec![Event::Text(text.clone())]);
                Ok(2)
            }
            Step::Action(a) => {
                app.dispatch(a.clone());
                Ok(2)
            }
            Step::Assert(check) => {
                run_check(self, app, check)?;
                Ok(0)
            }
            Step::Remember(tag) => {
                self.remembered.insert(tag.clone(), app.doc.clone());
                Ok(0)
            }
            Step::Screenshot { path, probes } => {
                let full = self.out_dir.join(path);
                app.screenshots.request(full.clone(), 0, false);
                self.phase = Phase::WaitingScreenshot { path: full, probes: probes.clone() };
                Err(String::new()) // handled by the phase, not an error
            }
            Step::Log(msg) => {
                log::info!("script: {msg}");
                Ok(0)
            }
        })();
        match result {
            Ok(0) if self.queue.is_empty() => self.next_step("ok"),
            Ok(n) => self.phase = Phase::Settling(n),
            Err(e) if e.is_empty() => {}
            Err(e) => self.fail(e),
        }
    }

    pub fn write_report(&self) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.out_dir)?;
        let path = self.out_dir.join("report.json");
        let mut report = self.report.clone();
        report.passed = self.finished == Some(true);
        std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
        Ok(path)
    }
}

fn find(app: &ShapeshopApp, name: &str) -> Result<ss_doc::ObjectId, String> {
    app.doc.objects().find(|(_, o)| o.name == name).map(|(id, _)| id).ok_or_else(|| format!("no object named {name:?}"))
}

fn run_check(runner: &ScriptRunner, app: &mut ShapeshopApp, check: &Check) -> Result<(), String> {
    let expect = |ok: bool, msg: String| if ok { Ok(()) } else { Err(msg) };
    match check {
        Check::ObjectCount(n) => {
            expect(app.doc.object_count() == *n, format!("expected {n} objects, found {}", app.doc.object_count()))
        }
        Check::SelectedCount(n) => {
            expect(app.sel.objects.len() == *n, format!("expected {n} picked, found {}", app.sel.objects.len()))
        }
        Check::SelectedName(name) => {
            let got = app.sel.active().and_then(|id| app.doc.object(id)).map(|o| o.name.clone());
            expect(got.as_deref() == Some(name.as_str()), format!("expected {name:?} picked, found {got:?}"))
        }
        Check::ObjectPosition { name, position, tolerance } => {
            let id = find(app, name)?;
            let p = app.doc.world_transform(id).translation;
            let want = Vec3::new(position.0, position.1, position.2);
            expect((p - want).abs().max_element() <= *tolerance, format!("{name} is at {p}, expected {want}"))
        }
        Check::ObjectSize { name, size, tolerance } => {
            let id = find(app, name)?;
            let s = app.eval.shape_mesh(&app.doc, id).ok_or("not a shape")?.bounds().size();
            let want = Vec3::new(size.0, size.1, size.2);
            expect((s - want).abs().max_element() <= *tolerance, format!("{name} is {s} big, expected {want}"))
        }
        Check::HistoryLen(n) => {
            expect(app.history.cursor() == *n, format!("expected {n} history steps, found {}", app.history.cursor()))
        }
        Check::CanUndo(b) => expect(app.history.can_undo() == *b, format!("can_undo is {}", app.history.can_undo())),
        Check::Dirty(b) => expect(app.history.is_dirty() == *b, format!("dirty is {}", app.history.is_dirty())),
        Check::ToolIs(t) => expect(app.tool.label().eq_ignore_ascii_case(t), format!("tool is {}", app.tool.label())),
        Check::SameAs(tag) => {
            let doc = runner.remembered.get(tag).ok_or_else(|| format!("nothing remembered as {tag:?}"))?;
            let same = doc.objects().count() == app.doc.objects().count()
                && doc.objects().zip(app.doc.objects()).all(|(a, b)| a.0 == b.0 && a.1 == b.1)
                && doc.roots() == app.doc.roots();
            expect(same, format!("document differs from {tag:?}"))
        }
        Check::AnchorExists(id) => expect(app.anchors.get(id).is_some(), format!("no UI element {id:?}")),
        Check::FileExists(p) => expect(Path::new(p).exists(), format!("{p} does not exist")),
        Check::Controls(c) => {
            expect(app.prefs.controls.key() == c, format!("controls are {}", app.prefs.controls.key()))
        }
        Check::Units(u) => {
            expect(app.doc.meta().units.short() == u, format!("units are {}", app.doc.meta().units.short()))
        }
        Check::StatusContains(s) => {
            expect(app.status_text().contains(s.as_str()), format!("status is {:?}", app.status_text()))
        }
    }
}

fn check_probes(runner: &ScriptRunner, app: &mut ShapeshopApp, path: &Path, probes: &[Probe]) -> Result<(), String> {
    if probes.is_empty() {
        return Ok(());
    }
    let img = image::open(path).map_err(|e| format!("can't read screenshot {}: {e}", path.display()))?.to_rgba8();
    let ppp = app.pixels_per_point;
    for probe in probes {
        let p = runner.resolve(app, &probe.at)?;
        let (x, y) = ((p.x * ppp).round() as u32, (p.y * ppp).round() as u32);
        let px = img.get_pixel_checked(x, y).ok_or_else(|| format!("probe {p:?} is outside the screenshot"))?;
        let d =
            [px[0] as i32 - probe.rgb.0 as i32, px[1] as i32 - probe.rgb.1 as i32, px[2] as i32 - probe.rgb.2 as i32];
        let close = d.iter().all(|c| c.unsigned_abs() <= probe.tolerance as u32);
        if close == probe.not {
            return Err(format!(
                "pixel at {p:?} is {:?}, expected {}{:?} (±{})",
                [px[0], px[1], px[2]],
                if probe.not { "not " } else { "" },
                probe.rgb,
                probe.tolerance
            ));
        }
    }
    Ok(())
}
