#![cfg(target_os = "macos")]

use std::ptr::NonNull;
use std::sync::{Arc, Mutex, OnceLock};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, msg_send_id, ClassType, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSEventMask, NSTextInputClient, NSTextInputContext, NSView};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSPoint, NSRange, NSRect, NSSize,
    NSString, NSUInteger,
};

use super::{Rect, TextRange};

// ── Shared state between ObjC view and Rust ─────────────────────

#[derive(Debug, Default)]
pub struct NativeImeState {
    pub committed: Vec<String>,
    pub marked_text: Option<String>,
    pub active: bool,
    pub candidate_rect: NSRect,
}

type SharedImeState = Arc<Mutex<NativeImeState>>;

thread_local! {
    static IME_STATE: std::cell::RefCell<Option<SharedImeState>> = const { std::cell::RefCell::new(None) };
}

fn with_ime_state<R>(f: impl FnOnce(&SharedImeState) -> R) -> Option<R> {
    IME_STATE.with(|cell| cell.borrow().as_ref().map(|s| f(s)))
}

// ── NSView subclass implementing NSTextInputClient ──────────────

define_class!(
    #[unsafe(super(NSView))]
    #[name = "AtermImeView"]
    #[thread_kind = MainThreadOnly]
    struct AtermImeView;

    unsafe impl NSTextInputClient for AtermImeView {
        #[unsafe(method(insertText:replacementRange:))]
        unsafe fn insertText_replacementRange(&self, string: &AnyObject, _range: NSRange) {
            let text: Option<Retained<NSString>> = unsafe { msg_send_id![string, string] };
            let text_str = text
                .map(|s| s.to_string())
                .or_else(|| {
                    let desc: Option<Retained<NSString>> =
                        unsafe { msg_send_id![string, description] };
                    desc.map(|s| s.to_string())
                })
                .unwrap_or_default();

            eprintln!("[NATIVE-IME] insertText: {:?}", text_str);

            if !text_str.is_empty() {
                with_ime_state(|state| {
                    if let Ok(mut s) = state.lock() {
                        s.committed.push(text_str);
                        s.marked_text = None;
                    }
                });
            }
        }

        #[unsafe(method(doCommandBySelector:))]
        unsafe fn doCommandBySelector(&self, _sel: Sel) {}

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        unsafe fn setMarkedText_selectedRange_replacementRange(
            &self,
            string: &AnyObject,
            _selected: NSRange,
            _replacement: NSRange,
        ) {
            let text: Option<Retained<NSString>> = unsafe { msg_send_id![string, string] };
            let text_str = text
                .map(|s| s.to_string())
                .or_else(|| {
                    let desc: Option<Retained<NSString>> =
                        unsafe { msg_send_id![string, description] };
                    desc.map(|s| s.to_string())
                })
                .unwrap_or_default();

            eprintln!("[NATIVE-IME] setMarkedText: {:?}", text_str);

            with_ime_state(|state| {
                if let Ok(mut s) = state.lock() {
                    s.marked_text = if text_str.is_empty() {
                        None
                    } else {
                        Some(text_str)
                    };
                }
            });
        }

        #[unsafe(method(unmarkText))]
        fn unmarkText(&self) {
            eprintln!("[NATIVE-IME] unmarkText");
            with_ime_state(|state| {
                if let Ok(mut s) = state.lock() {
                    s.marked_text = None;
                }
            });
        }

        #[unsafe(method(hasMarkedText))]
        fn hasMarkedText(&self) -> bool {
            with_ime_state(|state| {
                state
                    .lock()
                    .ok()
                    .map(|s| s.marked_text.is_some())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
        }

        #[unsafe(method(selectedRange))]
        fn selectedRange(&self) -> NSRange {
            NSRange::new(0, 0)
        }

        #[unsafe(method(markedRange))]
        fn markedRange(&self) -> NSRange {
            let len = with_ime_state(|state| {
                state
                    .lock()
                    .ok()
                    .and_then(|s| s.marked_text.as_ref().map(|t| t.len()))
            })
            .flatten()
            .unwrap_or(0);

            if len > 0 {
                NSRange::new(0, len)
            } else {
                NSRange::new(usize::MAX, 0)
            }
        }

        #[unsafe(method(attributedSubstringForProposedRange:actualRange:))]
        unsafe fn attributedSubstringForProposedRange_actualRange(
            &self,
            _range: NSRange,
            _actual_range: *mut NSRange,
        ) -> *mut NSAttributedString {
            std::ptr::null_mut()
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn validAttributesForMarkedText(&self) -> Retained<NSArray> {
            NSArray::new()
        }

        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        unsafe fn firstRectForCharacterRange_actualRange(
            &self,
            _range: NSRange,
            _actual_range: *mut NSRange,
        ) -> NSRect {
            with_ime_state(|state| {
                state
                    .lock()
                    .ok()
                    .map(|s| s.candidate_rect)
                    .unwrap_or_default()
            })
            .unwrap_or_default()
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn characterIndexForPoint(&self, _point: NSPoint) -> NSUInteger {
            0
        }
    }
);

impl AtermImeView {
    fn new_view(mtm: MainThreadMarker) -> Retained<Self> {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        unsafe { msg_send_id![Self::alloc(mtm), initWithFrame: frame] }
    }
}

// ── Public API ──────────────────────────────────────────────────

static HANDLER: OnceLock<NativeImeHandler> = OnceLock::new();

pub struct NativeImeHandler {
    state: SharedImeState,
    _monitor: Retained<AnyObject>,
    context: Retained<NSTextInputContext>,
}

unsafe impl Send for NativeImeHandler {}
unsafe impl Sync for NativeImeHandler {}

impl NativeImeHandler {
    pub fn initialize() -> &'static Self {
        HANDLER.get_or_init(|| {
            let mtm = MainThreadMarker::new()
                .expect("NativeImeHandler::initialize must be called from the main thread");

            let state: SharedImeState = Arc::new(Mutex::new(NativeImeState::default()));

            IME_STATE.with(|cell| {
                *cell.borrow_mut() = Some(state.clone());
            });

            let view = AtermImeView::new_view(mtm);

            let client = ProtocolObject::from_ref(&*view);
            let context = NSTextInputContext::initWithClient(
                NSTextInputContext::alloc(mtm),
                client,
            );

            let ctx = context.clone();
            let monitor_state = state.clone();
            let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                let event_ref = unsafe { event.as_ref() };

                let active = monitor_state
                    .lock()
                    .ok()
                    .map(|s| s.active)
                    .unwrap_or(false);

                if !active {
                    return event.as_ptr();
                }

                let handled = ctx.handleEvent(event_ref);

                if handled {
                    std::ptr::null_mut()
                } else {
                    event.as_ptr()
                }
            });

            let mask = NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged;
            let monitor = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block)
            }
            .expect("Failed to install NSEvent monitor");

            NativeImeHandler {
                state,
                _monitor: monitor,
                context,
            }
        })
    }

    pub fn set_active(&self, active: bool) {
        if let Ok(mut s) = self.state.lock() {
            s.active = active;
        }
        if active {
            self.context.activate();
        } else {
            self.context.deactivate();
        }
    }

    pub fn drain_committed(&self) -> Vec<String> {
        self.state
            .lock()
            .ok()
            .map(|mut s| std::mem::take(&mut s.committed))
            .unwrap_or_default()
    }

    pub fn marked_text(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.marked_text.clone())
    }

    pub fn set_candidate_rect(&self, rect: NSRect) {
        if let Ok(mut s) = self.state.lock() {
            s.candidate_rect = rect;
        }
        self.context.invalidateCharacterCoordinates();
    }

    pub fn is_active(&self) -> bool {
        self.state
            .lock()
            .ok()
            .map(|s| s.active)
            .unwrap_or(false)
    }
}

// ── Conversion impls ────────────────────────────────────────────

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
