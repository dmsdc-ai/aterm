use std::fmt;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::NativeImeHandler;

/// A portable text range used by the IME bridge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextRange {
    pub location: usize,
    pub length: usize,
}

impl TextRange {
    pub const fn new(location: usize, length: usize) -> Self {
        Self { location, length }
    }

    pub const fn empty(location: usize) -> Self {
        Self {
            location,
            length: 0,
        }
    }

    pub const fn end(self) -> usize {
        self.location + self.length
    }
}

/// A portable rectangle used for candidate window anchoring.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }
}

/// The current marked text shown by the IME.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkedTextState {
    pub text: String,
    pub selected_range: TextRange,
    pub replacement_range: TextRange,
}

/// The candidate rect returned to the platform text system.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CandidateRect {
    pub rect: Rect,
    pub actual_range: TextRange,
}

/// Snapshot of the bridge state, useful for later NSView integration.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImeSnapshot {
    pub marked_text: Option<MarkedTextState>,
    pub committed_text: Vec<String>,
    pub candidate_rect: Option<CandidateRect>,
}

#[derive(Debug, Default)]
struct ImeState {
    snapshot: ImeSnapshot,
}

/// Shared IME bridge state.
///
/// This is intentionally platform-neutral so the eventual NSView integration
/// can forward platform callbacks into a single Rust owner.
#[derive(Clone, Debug, Default)]
pub struct ImeBridge {
    inner: Arc<Mutex<ImeState>>,
}

impl ImeBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ImeSnapshot {
        self.inner
            .lock()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default()
    }

    pub fn set_candidate_rect(&self, candidate_rect: CandidateRect) {
        if let Ok(mut state) = self.inner.lock() {
            state.snapshot.candidate_rect = Some(candidate_rect);
        }
    }

    pub fn clear_candidate_rect(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.snapshot.candidate_rect = None;
        }
    }

    pub fn set_marked_text(
        &self,
        text: impl fmt::Display,
        selected_range: TextRange,
        replacement_range: TextRange,
    ) {
        if let Ok(mut state) = self.inner.lock() {
            state.snapshot.marked_text = Some(MarkedTextState {
                text: text.to_string(),
                selected_range,
                replacement_range,
            });
        }
    }

    pub fn insert_text(&self, text: impl fmt::Display, replacement_range: TextRange) {
        if let Ok(mut state) = self.inner.lock() {
            let rect = state
                .snapshot
                .candidate_rect
                .map(|candidate| candidate.rect)
                .unwrap_or_else(Rect::zero);
            state.snapshot.committed_text.push(text.to_string());
            state.snapshot.marked_text = None;
            state.snapshot.candidate_rect = Some(CandidateRect {
                rect,
                actual_range: replacement_range,
            });
        }
    }

    pub fn unmark_text(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.snapshot.marked_text = None;
        }
    }

    pub fn first_rect_for_character_range(&self, range: TextRange) -> CandidateRect {
        self.snapshot().candidate_rect.unwrap_or(CandidateRect {
            rect: Rect::zero(),
            actual_range: range,
        })
    }
}

#[cfg(not(target_os = "macos"))]
impl ImeBridge {
    pub fn set_marked_text_non_macos(
        &self,
        text: impl fmt::Display,
        selected_range: TextRange,
        replacement_range: TextRange,
    ) {
        self.set_marked_text(text, selected_range, replacement_range);
    }

    pub fn insert_text_non_macos(&self, text: impl fmt::Display, replacement_range: TextRange) {
        self.insert_text(text, replacement_range);
    }

    pub fn first_rect_for_character_range_non_macos(&self, range: TextRange) -> CandidateRect {
        self.first_rect_for_character_range(range)
    }
}
