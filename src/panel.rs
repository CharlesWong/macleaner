//! The native panel: a borderless, transparent `NSPanel` hosting a `WKWebView`
//! that renders `bridge::PANEL_HTML`, with a `WKScriptMessageHandler` bridge so
//! the WebView's buttons reach Rust. All calls here run on the main thread.

use crate::bridge::PANEL_HTML;
use block2::RcBlock;
use core::ptr::NonNull;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSEventMask, NSPanel, NSScreen, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSObject, NSObjectProtocol, NSNumber, NSPoint, NSRect, NSSize, NSString,
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
    /// Wakes the event loop (with `()`) when a click outside the app dismisses
    /// the panel, so the loop hides + tears it down promptly.
    proxy: EventLoopProxy<()>,
    /// The active global click-outside monitor (only while visible).
    monitor: Option<Retained<AnyObject>>,
    pub visible: bool,
}

impl Panel {
    pub fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<()>) -> Panel {
        Panel { inner: None, mtm, proxy, monitor: None, visible: false }
    }

    /// Install a global monitor that fires on any mouse-down OUTSIDE our app
    /// (global monitors never see our own windows), waking the loop to dismiss.
    fn install_monitor(&mut self) {
        if self.monitor.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        let block = RcBlock::new(move |_event: NonNull<NSEvent>| {
            let _ = proxy.send_event(());
        });
        let mask = NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown;
        self.monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &block);
    }

    fn remove_monitor(&mut self) {
        if let Some(m) = self.monitor.take() {
            // SAFETY: `m` was returned by addGlobalMonitorForEvents…; removing it
            // exactly once is correct.
            unsafe { NSEvent::removeMonitor(&m) };
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
        let mtm = self.mtm;
        let inner = self.inner.get_or_insert_with(|| Self::build(mtm));
        Self::position(&inner.window, mtm, center_x_px);
        inner.window.orderFrontRegardless();
        self.visible = true;
        self.install_monitor();
    }

    pub fn hide(&mut self) {
        self.remove_monitor();
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
        self.remove_monitor();
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
