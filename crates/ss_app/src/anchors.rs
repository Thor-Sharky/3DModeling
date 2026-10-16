//! Named screen rectangles of UI elements ("shapes.block", "top.export", ...).
//!
//! Widgets register themselves every frame. Scripts click them by name, quests
//! spotlight them, and "Show me" points at them. Lookups use the previous
//! frame's rectangles, which are complete by the time anyone asks.

use std::collections::HashMap;

use eframe::egui::Rect;

#[derive(Default, Debug)]
pub struct Anchors {
    current: HashMap<String, Rect>,
    previous: HashMap<String, Rect>,
}

impl Anchors {
    /// Call at the start of every frame.
    pub fn begin_frame(&mut self) {
        self.previous = std::mem::take(&mut self.current);
    }

    pub fn register(&mut self, id: impl Into<String>, rect: Rect) {
        self.current.insert(id.into(), rect);
    }

    /// Where an element was last drawn.
    pub fn get(&self, id: &str) -> Option<Rect> {
        self.current.get(id).or_else(|| self.previous.get(id)).copied()
    }

    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.previous.keys()
    }
}
