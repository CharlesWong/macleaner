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
    NSBackingStoreType, NSColor, NSEvent, NSEventMask, NSPanel, NSScreen,
    NSWindowCollectionBehavior, NSWindowDidResignKeyNotification, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect,
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

define_class!(
    // A borderless NSPanel returns canBecomeKeyWindow=false by default, so it
    // could never become key — which broke resign-key dismissal and Escape.
    // This subclass overrides it so the panel becomes key (still non-activating).
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "MacleanerKeyPanel"]
    struct KeyPanel;

    unsafe impl NSObjectProtocol for KeyPanel {}

    impl KeyPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
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
    /// Local event monitor (Escape key) active while visible.
    monitors: Vec<Retained<AnyObject>>,
    /// resign-key observer handle (dismiss on outside click / Cmd-Tab).
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
            observer: None,
            visible: false,
        }
    }

    /// Install the dismiss triggers: Escape (local key monitor) and resign-key
    /// (the panel is key, so clicking outside / Cmd-Tab resigns it → dismiss).
    fn install_dismiss(&mut self, panel_win: &Retained<NSPanel>) {
        // Escape key → dismiss + swallow the key.
        let p_key = self.proxy.clone();
        let key_block = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
            let e = unsafe { ev.as_ref() };
            if e.keyCode() == KEY_ESCAPE {
                let _ = p_key.send_event(());
                return ptr::null_mut();
            }
            ev.as_ptr()
        });
        if let Some(m) = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &key_block)
        } {
            self.monitors.push(m);
        }

        // resign-key: the panel is key, so clicking another app/window or the
        // desktop, or Cmd-Tab, resigns its key status → dismiss. Filtered to OUR
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
        // Instantiate the key-capable subclass via NSPanel's designated
        // initializer (return type must match the alloc's class), then upcast.
        let kp: Retained<KeyPanel> = unsafe {
            msg_send![
                KeyPanel::alloc(mtm),
                initWithContentRect: frame,
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        let window: Retained<NSPanel> = kp.into_super();
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

        Inner {
            window,
            webview,
            rx,
            _handler: handler,
        }
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
        // Make the panel key (non-activating) so resign-key dismisses it.
        panel_win.makeKeyAndOrderFront(None);
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
