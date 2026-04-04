#![cfg(target_os = "macos")]
//! Native macOS IME bridge using a hidden `NSView` attached to the real winit window.
//!
//! The hidden responder owns the `NSTextInputClient` path, while the Rust app polls
//! committed text and command bytes and forwards them to the PTY.

use std::sync::{Arc, Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel, ClassType};
use objc2_app_kit::{NSTextInputClient, NSView, NSWindow};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString,
    NSUInteger,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{Rect, TextRange};

// ── Shared state ────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ImeState {
    pub committed: Vec<String>,
    pub marked_text: Option<String>,
    pub key_bytes: Vec<Vec<u8>>,
    pub candidate_rect: NSRect,
    pub accumulator: Option<Vec<String>>,
}

type SharedState = Arc<Mutex<ImeState>>;

static IME_SHARED: OnceLock<SharedState> = OnceLock::new();

fn shared() -> &'static SharedState {
    IME_SHARED.get_or_init(|| Arc::new(Mutex::new(ImeState::default())))
}

fn extract_string(obj: &AnyObject) -> String {
    let is_attr: bool = unsafe { msg_send![obj, isKindOfClass: NSAttributedString::class()] };
    if is_attr {
        let attr: &NSAttributedString =
            unsafe { &*(obj as *const AnyObject as *const NSAttributedString) };
        attr.string().to_string()
    } else {
        let ns: &NSString = unsafe { &*(obj as *const AnyObject as *const NSString) };
        ns.to_string()
    }
}

fn selector_to_pty_bytes(sel: Sel) -> Option<Vec<u8>> {
    let name = sel.name().to_str().ok()?;
    match name {
        "insertNewline:" => Some(b"\r".to_vec()),
        "insertTab:" => Some(b"\t".to_vec()),
        "deleteBackward:" => Some(vec![0x7f]),
        "deleteForward:" => Some(b"\x1b[3~".to_vec()),
        "moveUp:" => Some(b"\x1b[A".to_vec()),
        "moveDown:" => Some(b"\x1b[B".to_vec()),
        "moveRight:" => Some(b"\x1b[C".to_vec()),
        "moveLeft:" => Some(b"\x1b[D".to_vec()),
        "moveToBeginningOfLine:" => Some(b"\x1b[H".to_vec()),
        "moveToEndOfLine:" => Some(b"\x1b[F".to_vec()),
        "cancelOperation:" => Some(vec![0x1b]),
        "insertBacktab:" => Some(b"\x1b[Z".to_vec()),
        _ => None,
    }
}

// ── extern "C" methods for the ObjC class ───────────────────────

extern "C" fn accepts_first_responder(_this: *mut AnyObject, _cmd: Sel) -> Bool {
    Bool::YES
}

extern "C" fn can_become_key_view(_this: *mut AnyObject, _cmd: Sel) -> Bool {
    Bool::YES
}

extern "C" fn key_down(this: *mut AnyObject, _cmd: Sel, event: *mut AnyObject) {
    if event.is_null() {
        return;
    }

    // Check Cmd/Ctrl → forward to super (shortcuts like Cmd+C)
    let flags: usize = unsafe { msg_send![event, modifierFlags] };
    let cmd_flag: usize = 1 << 20;
    let ctrl_flag: usize = 1 << 18;
    if (flags & cmd_flag) != 0 || (flags & ctrl_flag) != 0 {
        // Forward to next responder (winit handles shortcuts)
        let next: *mut AnyObject = unsafe { msg_send![this, nextResponder] };
        if !next.is_null() {
            let _: () = unsafe { msg_send![next, keyDown: event] };
        }
        return;
    }

    eprintln!("[ime] keyDown (native handler)");

    // Ghostty accumulator: activate before interpretKeyEvents
    if let Ok(mut s) = shared().lock() {
        s.accumulator = Some(Vec::new());
    }

    // Route through macOS IME
    let events = unsafe { NSArray::from_retained_slice(&[Retained::retain(event).unwrap()]) };
    let _: () = unsafe { msg_send![this, interpretKeyEvents: &*events] };

    // Drain accumulator → committed
    let accumulated = shared()
        .lock()
        .ok()
        .and_then(|mut s| s.accumulator.take())
        .unwrap_or_default();

    if !accumulated.is_empty() {
        if let Ok(mut s) = shared().lock() {
            for text in accumulated {
                if !text.is_empty() {
                    s.committed.push(text);
                }
            }
        }
    }
}

extern "C" fn insert_text(
    _this: *mut AnyObject,
    _cmd: Sel,
    string: *mut AnyObject,
    _range: NSRange,
) {
    if string.is_null() {
        return;
    }
    let text = extract_string(unsafe { &*string });
    eprintln!("[ime] insertText: {:?}", text);
    if text.is_empty() {
        return;
    }
    if let Ok(mut s) = shared().lock() {
        if let Some(ref mut acc) = s.accumulator {
            acc.push(text); // Inside keyDown → accumulate
        } else {
            s.committed.push(text); // Outside keyDown → commit directly
        }
        s.marked_text = None;
    }
}

extern "C" fn do_command_by_selector(_this: *mut AnyObject, _cmd: Sel, sel: Sel) {
    if let Some(bytes) = selector_to_pty_bytes(sel) {
        if let Ok(mut s) = shared().lock() {
            s.key_bytes.push(bytes);
            s.accumulator = None; // Command means not text input
        }
    }
}

extern "C" fn set_marked_text(
    _this: *mut AnyObject,
    _cmd: Sel,
    string: *mut AnyObject,
    _selected: NSRange,
    _replacement: NSRange,
) {
    if string.is_null() {
        return;
    }
    let text = extract_string(unsafe { &*string });
    eprintln!("[ime] setMarkedText: {:?}", text);
    if let Ok(mut s) = shared().lock() {
        s.marked_text = if text.is_empty() { None } else { Some(text) };
    }
}

extern "C" fn unmark_text(_this: *mut AnyObject, _cmd: Sel) {
    if let Ok(mut s) = shared().lock() {
        s.marked_text = None;
    }
}

extern "C" fn has_marked_text(_this: *mut AnyObject, _cmd: Sel) -> Bool {
    let val = shared()
        .lock()
        .ok()
        .map(|s| s.marked_text.is_some())
        .unwrap_or(false);
    Bool::new(val)
}

extern "C" fn selected_range(_this: *mut AnyObject, _cmd: Sel) -> NSRange {
    NSRange::new(0, 0)
}

extern "C" fn marked_range(_this: *mut AnyObject, _cmd: Sel) -> NSRange {
    let len = shared()
        .lock()
        .ok()
        .and_then(|s| s.marked_text.as_ref().map(|t| t.chars().count()))
        .unwrap_or(0);
    if len > 0 {
        NSRange::new(0, len)
    } else {
        NSRange::new(isize::MAX as usize, 0)
    }
}

extern "C" fn attributed_substring(
    _this: *mut AnyObject,
    _cmd: Sel,
    _range: NSRange,
    _actual: *mut NSRange,
) -> *const AnyObject {
    std::ptr::null()
}

extern "C" fn valid_attributes(_this: *mut AnyObject, _cmd: Sel) -> *const AnyObject {
    let arr = NSArray::<AnyObject>::new();
    Retained::into_raw(arr) as *const AnyObject
}

extern "C" fn first_rect(
    _this: *mut AnyObject,
    _cmd: Sel,
    _range: NSRange,
    _actual: *mut NSRange,
) -> NSRect {
    shared()
        .lock()
        .ok()
        .map(|s| s.candidate_rect)
        .unwrap_or_default()
}

extern "C" fn character_index(_this: *mut AnyObject, _cmd: Sel, _point: NSPoint) -> NSUInteger {
    0
}

// ── Class registration ──────────────────────────────────────────

fn register_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let superclass = NSView::class();
        let mut builder = ClassBuilder::new(c"HiddenImeView", superclass)
            .expect("Failed to create HiddenImeView class");

        unsafe {
            // NSResponder overrides
            builder.add_method(
                sel!(acceptsFirstResponder),
                accepts_first_responder as extern "C" fn(*mut AnyObject, Sel) -> Bool,
            );
            builder.add_method(
                sel!(canBecomeKeyView),
                can_become_key_view as extern "C" fn(*mut AnyObject, Sel) -> Bool,
            );
            builder.add_method(
                sel!(keyDown:),
                key_down as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );

            // NSTextInputClient protocol
            builder.add_method(
                sel!(insertText:replacementRange:),
                insert_text as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, NSRange),
            );
            builder.add_method(
                sel!(doCommandBySelector:),
                do_command_by_selector as extern "C" fn(*mut AnyObject, Sel, Sel),
            );
            builder.add_method(
                sel!(setMarkedText:selectedRange:replacementRange:),
                set_marked_text
                    as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, NSRange, NSRange),
            );
            builder.add_method(
                sel!(unmarkText),
                unmark_text as extern "C" fn(*mut AnyObject, Sel),
            );
            builder.add_method(
                sel!(hasMarkedText),
                has_marked_text as extern "C" fn(*mut AnyObject, Sel) -> Bool,
            );
            builder.add_method(
                sel!(selectedRange),
                selected_range as extern "C" fn(*mut AnyObject, Sel) -> NSRange,
            );
            builder.add_method(
                sel!(markedRange),
                marked_range as extern "C" fn(*mut AnyObject, Sel) -> NSRange,
            );
            builder.add_method(
                sel!(attributedSubstringForProposedRange:actualRange:),
                attributed_substring
                    as extern "C" fn(
                        *mut AnyObject,
                        Sel,
                        NSRange,
                        *mut NSRange,
                    ) -> *const AnyObject,
            );
            builder.add_method(
                sel!(validAttributesForMarkedText),
                valid_attributes as extern "C" fn(*mut AnyObject, Sel) -> *const AnyObject,
            );
            builder.add_method(
                sel!(firstRectForCharacterRange:actualRange:),
                first_rect as extern "C" fn(*mut AnyObject, Sel, NSRange, *mut NSRange) -> NSRect,
            );
            builder.add_method(
                sel!(characterIndexForPoint:),
                character_index as extern "C" fn(*mut AnyObject, Sel, NSPoint) -> NSUInteger,
            );

            // Declare NSTextInputClient protocol conformance
            if let Some(protocol) = objc2::runtime::AnyProtocol::get(c"NSTextInputClient") {
                builder.add_protocol(protocol);
            }
        }

        builder.register()
    })
}

// ── Public API ──────────────────────────────────────────────────

static HANDLER: OnceLock<NativeImeHandler> = OnceLock::new();

pub struct NativeImeHandler {
    /// Raw pointer to our HiddenImeView instance (retained via ObjC runtime).
    view: *mut AnyObject,
    attached_host_view: Mutex<Option<usize>>,
}

unsafe impl Send for NativeImeHandler {}
unsafe impl Sync for NativeImeHandler {}

impl NativeImeHandler {
    pub fn initialize() -> &'static Self {
        HANDLER.get_or_init(|| {
            let _mtm = MainThreadMarker::new()
                .expect("NativeImeHandler must be initialized on main thread");

            // Ensure shared state is initialized
            let _ = shared();

            // Create the view via ObjC runtime (alloc + initWithFrame:)
            let cls = register_class();
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
            let view: *mut AnyObject = unsafe { msg_send![cls, alloc] };
            let view: *mut AnyObject = unsafe { msg_send![view, initWithFrame: frame] };

            NativeImeHandler {
                view,
                attached_host_view: Mutex::new(None),
            }
        })
    }

    fn hidden_view(&self) -> &NSView {
        unsafe { &*(self.view.cast::<NSView>()) }
    }

    fn appkit_handles<W: HasWindowHandle>(
        window: &W,
    ) -> Option<(Retained<NSView>, Retained<NSWindow>)> {
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };

        let host_view = unsafe { Retained::retain(appkit.ns_view.as_ptr().cast::<NSView>()) }?;
        let ns_window = host_view.window()?;
        Some((host_view, ns_window))
    }

    pub fn attach_to_window<W: HasWindowHandle>(&self, window: &W) -> bool {
        let Some((host_view, _)) = Self::appkit_handles(window) else {
            eprintln!("[ime/native] failed to resolve AppKit handles");
            return false;
        };

        let host_ptr = Retained::as_ptr(&host_view) as usize;
        let hidden_view = self.hidden_view();

        if let Ok(mut attached_host_view) = self.attached_host_view.lock() {
            if attached_host_view.as_ref() == Some(&host_ptr) {
                return true;
            }

            hidden_view.removeFromSuperview();
            hidden_view.setHidden(true);
            host_view.addSubview(hidden_view);
            *attached_host_view = Some(host_ptr);
        }

        true
    }

    pub fn activate_for_window<W: HasWindowHandle>(&self, window: &W) -> bool {
        let Some((_host_view, ns_window)) = Self::appkit_handles(window) else {
            return false;
        };

        if !self.attach_to_window(window) {
            return false;
        }

        let became_first = ns_window.makeFirstResponder(Some(self.hidden_view()));
        if became_first {
            // Activate the view's built-in inputContext — the same one used by
            // interpretKeyEvents: — so macOS IME composition works correctly.
            let ctx: *mut AnyObject = unsafe { msg_send![self.view, inputContext] };
            if !ctx.is_null() {
                let _: () = unsafe { msg_send![ctx, activate] };
            }
        }
        became_first
    }

    pub fn deactivate_for_window<W: HasWindowHandle>(&self, window: &W) -> bool {
        let Some((host_view, ns_window)) = Self::appkit_handles(window) else {
            return false;
        };

        let ctx: *mut AnyObject = unsafe { msg_send![self.view, inputContext] };
        if !ctx.is_null() {
            let _: () = unsafe { msg_send![ctx, deactivate] };
        }
        ns_window.makeFirstResponder(Some(&host_view))
    }

    pub fn drain_committed(&self) -> Vec<String> {
        shared()
            .lock()
            .ok()
            .map(|mut s| std::mem::take(&mut s.committed))
            .unwrap_or_default()
    }

    pub fn drain_key_bytes(&self) -> Vec<Vec<u8>> {
        shared()
            .lock()
            .ok()
            .map(|mut s| std::mem::take(&mut s.key_bytes))
            .unwrap_or_default()
    }

    pub fn marked_text(&self) -> Option<String> {
        shared().lock().ok().and_then(|s| s.marked_text.clone())
    }

    pub fn set_candidate_rect(&self, rect: NSRect) {
        if let Ok(mut s) = shared().lock() {
            s.candidate_rect = rect;
        }
        let ctx: *mut AnyObject = unsafe { msg_send![self.view, inputContext] };
        if !ctx.is_null() {
            let _: () = unsafe { msg_send![ctx, invalidateCharacterCoordinates] };
        }
    }
}

// ── Conversion impls ────────────────────────────────────────────

impl From<TextRange> for NSRange {
    fn from(r: TextRange) -> Self {
        Self::new(r.location, r.length)
    }
}
impl From<NSRange> for TextRange {
    fn from(r: NSRange) -> Self {
        Self::new(r.location, r.length)
    }
}
impl From<Rect> for NSRect {
    fn from(r: Rect) -> Self {
        NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.width, r.height))
    }
}
impl From<NSRect> for Rect {
    fn from(r: NSRect) -> Self {
        Self::new(r.origin.x, r.origin.y, r.size.width, r.size.height)
    }
}
