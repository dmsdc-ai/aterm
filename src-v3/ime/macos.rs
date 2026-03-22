#![cfg(target_os = "macos")]

use std::fmt;

use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize};

use super::{CandidateRect, ImeBridge, Rect, TextRange};

impl ImeBridge {
    /// Helper for later `NSTextInputClient::setMarkedText:selectedRange:replacementRange:`.
    pub fn set_marked_text_ns(
        &self,
        text: impl fmt::Display,
        selected_range: NSRange,
        replacement_range: NSRange,
    ) {
        self.set_marked_text(text, selected_range.into(), replacement_range.into());
    }

    /// Helper for later `NSTextInputClient::insertText:replacementRange:`.
    pub fn insert_text_ns(&self, text: impl fmt::Display, replacement_range: NSRange) {
        self.insert_text(text, replacement_range.into());
    }

    /// Helper for later `NSTextInputClient::unmarkText`.
    pub fn unmark_text_ns(&self) {
        self.unmark_text();
    }

    /// Helper for later `NSTextInputClient::firstRectForCharacterRange:actualRange:`.
    pub fn first_rect_for_character_range_ns(&self, range: NSRange) -> (NSRect, NSRange) {
        let candidate = self.first_rect_for_character_range(range.into());
        (candidate.rect.into(), candidate.actual_range.into())
    }

    /// Store the next candidate window location in screen coordinates.
    pub fn set_candidate_rect_ns(&self, rect: NSRect, actual_range: NSRange) {
        self.set_candidate_rect(CandidateRect {
            rect: rect.into(),
            actual_range: actual_range.into(),
        });
    }
}

impl From<TextRange> for NSRange {
    fn from(range: TextRange) -> Self {
        Self::new(range.location, range.length)
    }
}

impl From<NSRange> for TextRange {
    fn from(range: NSRange) -> Self {
        Self::new(range.location, range.length)
    }
}

impl From<Rect> for NSRect {
    fn from(rect: Rect) -> Self {
        NSRect::new(
            NSPoint::new(rect.x, rect.y),
            NSSize::new(rect.width, rect.height),
        )
    }
}

impl From<NSRect> for Rect {
    fn from(rect: NSRect) -> Self {
        Self::new(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
    }
}
