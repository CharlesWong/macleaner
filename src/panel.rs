//! The native panel: a borderless, transparent `NSPanel` hosting a `WKWebView`
//! that renders `bridge::PANEL_HTML`, with a `WKScriptMessageHandler` bridge so
//! the WebView's buttons reach Rust. All calls here run on the main thread.

use crate::bridge::PANEL_HTML;
use block2::RcBlock;
use core::ptr::{self, NonNull};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSEventMask, NSPanel, NSScreen, NSWindow,
    NSWindowCollectionBehavior, NSWindowDidResignKeyNotification, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSNumber, NSPoint, NSRect,
    NSSize, NSString,
};
use objc2_web_kit::{
    WKScriptMessage, WKScriptMessageHandler, WKUserContentController, WKWebView,
    WKWebViewConfiguration,
};
use std::sync::mpsc::{channel, Receiver, Sender};
use tao::event_loop::EventLoopProxy;

const WIN_W: f64 = 332.0;
const WIN_H: f64 = 540.0;

/// Ivars for the JS→native message handler: a channel carrying raw JSON bodies.
struct HandlerIvars {
    tx: Sender<String>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MacleanerMsgHandler"]
    #[ivars = HandlerIvars]
    struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    unsafe impl WKScriptMessageHandler for Handler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _ucc: &WKUserContentController, message: &WKScriptMessage) {
            let body = unsafe { message.body() };
            if let Ok(s) = body.downcast::<NSString>() {
                let _ = self.ivars().tx.send(s.to_string());
            }
        }
    }
);

impl Handler {
    fn new(mtm: MainThreadMarker, tx: Sender<String>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HandlerIvars { tx });
        unsafe { msg_send![super(this), init] }
    }
}

/// The live window + webview + action channel. Built lazily on first open and
/// torn down when the panel is closed while idle, so a never-opened menu-bar app
/// holds ZERO WebKit processes.
struct Inner {
    window: Retained<NSPanel>,
    webview: Retained<WKWebView>,
    rx: Receiver<String>,
    _handler: Retained<Handler>,
}

pub struct Panel {
    inner: Option<Inner>,
    mtm: MainThreadMarker,
    /// Wakes the event loop (with `()`) to dismiss the panel.
    proxy: EventLoopProxy<()>,
    /// Local event monitors (Escape key + outside-click) active while visible.
    monitors: Vec<Retained<AnyObject>>,
    /// Transparent full-screen window that catches outside clicks so they become
    /// our-app events (the local monitor then dismisses + swallows them).
    catcher: Option<Retained<NSWindow>>,
    /// resign-key observer handle (dismiss on Cmd-Tab / app switch).
    observer: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    pub visible: bool,
}

/// macOS virtual key code for the Escape key.
const KEY_ESCAPE: u16 = 53;

impl Panel {
    pub fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<()>) -> Panel {
        Panel {
            inner: None,
            mtm,
            proxy,
            monitors: Vec::new(),
            catcher: None,
            observer: None,
            visible: false,
        }
    }

    /// A transparent, click-catching window covering all displays, sitting just
    /// below the panel. Invisible; its only job is to receive (and let us
    /// swallow) clicks outside the panel.
    fn build_catcher(mtm: MainThreadMarker) -> Retained<NSWindow> {
        let frame = NSRect::new(NSPoint::new(-5000.0, -5000.0), NSSize::new(20000.0, 20000.0));
        let w = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            w.setReleasedWhenClosed(false);
            w.setOpaque(false);
            w.setBackgroundColor(Some(&NSColor::clearColor()));
            w.setHasShadow(false);
            w.setIgnoresMouseEvents(false);
            w.setLevel(24); // just below the panel's status level (25)
        }
        w
    }

    /// Install the three dismiss triggers: Escape, outside-click (swallowed via
    /// the catcher + a local monitor), and resign-key (app switch).
    fn install_dismiss(&mut self, panel_win: &Retained<NSPanel>) {
        let mtm = self.mtm;

        // Escape key → dismiss + swallow.
        let p_key = self.proxy.clone();
        let key_block = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
            let e = unsafe { ev.as_ref() };
            if e.keyCode() == KEY_ESCAPE {
                let _ = p_key.send_event(());
                return ptr::null_mut();
            }
            ev.as_ptr()
        });
        if let Some(m) =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &key_block) }
        {
            self.monitors.push(m);
        }

        // Mouse-down: clicks inside the panel pass through; clicks on the catcher
        // (any other of our windows) dismiss AND are swallowed (return null); a
        // window-less event is ambiguous → pass through (never dismiss on it).
        let p_mouse = self.proxy.clone();
        let pw = panel_win.clone();
        let mouse_block = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
            let e = unsafe { ev.as_ref() };
            match e.window(mtm) {
                Some(w)
                    if ptr::eq(
                        Retained::as_ptr(&w) as *const (),
                        Retained::as_ptr(&pw) as *const (),
                    ) =>
                {
                    ev.as_ptr() // on the panel → pass through
                }
                Some(_) => {
                    let _ = p_mouse.send_event(()); // on the catcher → dismiss
                    ptr::null_mut() // …and swallow the click
                }
                None => ev.as_ptr(), // no window → ambiguous, don't dismiss
            }
        });
        let mmask = NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown;
        if let Some(m) =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mmask, &mouse_block) }
        {
            self.monitors.push(m);
        }

        // resign-key (Cmd-Tab / clicking another app) → dismiss. Filtered to OUR
        // panel window so an unrelated window losing key never dismisses us.
        let p_obs = self.proxy.clone();
        let obs_block = RcBlock::new(move |_n: NonNull<NSNotification>| {
            let _ = p_obs.send_event(());
        });
        // SAFETY: NSPanel is an objc object; the cast to &AnyObject is sound and
        // only used as the notification's object filter.
        let panel_any: &AnyObject = unsafe { &*(Retained::as_ptr(panel_win) as *const AnyObject) };
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowDidResignKeyNotification),
                Some(panel_any),
                None,
                &obs_block,
            )
        };
        self.observer = Some(observer);
    }

    fn remove_dismiss(&mut self) {
        for m in self.monitors.drain(..) {
            // SAFETY: each `m` was returned by addLocalMonitorForEvents…
            unsafe { NSEvent::removeMonitor(&m) };
        }
        if let Some(obs) = self.observer.take() {
            // SAFETY: `obs` was returned by addObserverForName…; ProtocolObject is
            // repr(transparent) over the objc object, so the cast to &AnyObject is sound.
            let any: &AnyObject = unsafe { &*(Retained::as_ptr(&obs) as *const AnyObject) };
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(any) };
        }
        if let Some(c) = self.catcher.take() {
            c.orderOut(None);
        }
    }

    /// Build the window + webview + bridge (spawns the WebKit processes).
    fn build(mtm: MainThreadMarker) -> Inner {
        let (tx, rx) = channel::<String>();
        let handler = Handler::new(mtm, tx);

        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        let ucc: Retained<WKUserContentController> = unsafe { config.userContentController() };
        unsafe {
            ucc.addScriptMessageHandler_name(
                ProtocolObject::from_ref(&*handler),
                &NSString::from_str("macleaner"),
            );
        }

        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIN_W, WIN_H));
        let webview =
            unsafe { WKWebView::initWithFrame_configuration(mtm.alloc(), frame, &config) };
        // Transparent WebView so the HTML card's own rounded corners + shadow show.
        let no = NSNumber::numberWithBool(false);
        let draws_key = NSString::from_str("drawsBackground");
        unsafe {
            let _: () = msg_send![&*webview, setValue: &*no, forKey: &*draws_key];
            webview.loadHTMLString_baseURL(&NSString::from_str(PANEL_HTML), None);
        }

        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe {
            // We own the window via `Retained`; don't let close() double-free it.
            window.setReleasedWhenClosed(false);
            window.setOpaque(false);
            window.setBackgroundColor(Some(&NSColor::clearColor()));
            window.setHasShadow(false);
            window.setLevel(25); // status-window level
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            let _: () = msg_send![&*window, setContentView: &*webview];
        }

        Inner { window, webview, rx, _handler: handler }
    }

    fn position(window: &NSPanel, mtm: MainThreadMarker, center_x_px: f64) {
        let Some(screen) = NSScreen::mainScreen(mtm) else {
            return;
        };
        let vf = screen.visibleFrame();
        let scale = screen.backingScaleFactor();
        let center_x_pt = center_x_px / scale.max(1.0);
        let mut x = center_x_pt - WIN_W / 2.0;
        let max_x = vf.origin.x + vf.size.width - WIN_W - 6.0;
        let min_x = vf.origin.x + 6.0;
        if x > max_x {
            x = max_x;
        }
        if x < min_x {
            x = min_x;
        }
        let y = vf.origin.y + vf.size.height - WIN_H - 2.0;
        window.setFrameOrigin(NSPoint::new(x, y));
    }

    /// Evaluate a JS snippet in the WebView (no-op if the panel isn't built).
    pub fn eval(&self, js: &str) {
        if let Some(inner) = &self.inner {
            unsafe {
                inner
                    .webview
                    .evaluateJavaScript_completionHandler(&NSString::from_str(js), None);
            }
        }
    }

    pub fn show(&mut self, center_x_px: f64) {
        if self.visible {
            return; // idempotent: never stack catchers/monitors/observers
        }
        let mtm = self.mtm;
        // Build (lazily) + position the panel, and grab a window handle.
        let panel_win = {
            let inner = self.inner.get_or_insert_with(|| Self::build(mtm));
            Self::position(&inner.window, mtm, center_x_px);
            inner.window.clone()
        };
        // Click-catcher below, panel key on top.
        let catcher = Self::build_catcher(mtm);
        catcher.orderFrontRegardless();
        panel_win.makeKeyAndOrderFront(None);
        self.catcher = Some(catcher);
        self.visible = true;
        self.install_dismiss(&panel_win);
    }

    pub fn hide(&mut self) {
        self.remove_dismiss();
        if let Some(inner) = &self.inner {
            inner.window.orderOut(None);
        }
        self.visible = false;
    }

    pub fn toggle(&mut self, center_x_px: f64) {
        if self.visible {
            self.hide();
        } else {
            self.show(center_x_px);
        }
    }

    /// Tear down the window + webview, terminating the WebKit processes. Safe to
    /// call repeatedly; a no-op once already released.
    pub fn release(&mut self) {
        self.remove_dismiss();
        if let Some(inner) = self.inner.take() {
            inner.window.orderOut(None);
            inner.window.setContentView(None); // drop the webview from the window
            // `inner` (window, webview, handler, rx) is dropped here.
        }
    }

    /// Whether the WebView is currently built (for measuring / tests).
    pub fn is_built(&self) -> bool {
        self.inner.is_some()
    }

    /// Drain any actions the WebView posted since the last poll.
    pub fn poll_actions(&self) -> Vec<String> {
        match &self.inner {
            Some(inner) => inner.rx.try_iter().collect(),
            None => Vec::new(),
        }
    }
}
