#![cfg(target_os = "macos")]

use std::ptr;

use block2::DynBlock;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel},
};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSTextField};
use objc2_foundation::NSString;
use objc2_web_kit::{WKFrameInfo, WKUIDelegate, WKWebView};
use wry::WebViewExtMacOS;

pub struct RabUIDelegateIvars {
    original: Option<Retained<ProtocolObject<dyn WKUIDelegate>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RabUIDelegateIvars]
    pub struct RabUIDelegate;

    unsafe impl NSObjectProtocol for RabUIDelegate {
        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> bool {
            let super_responds = unsafe { msg_send![super(self), respondsToSelector: selector] };
            super_responds
                || self
                    .ivars()
                    .original
                    .as_deref()
                    .is_some_and(|delegate| delegate.respondsToSelector(selector))
        }
    }

    unsafe impl WKUIDelegate for RabUIDelegate {
        #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_javascript_alert(
            &self,
            _webview: &WKWebView,
            message: &NSString,
            frame: &WKFrameInfo,
            completion_handler: &DynBlock<dyn Fn()>,
        ) {
            let alert = new_alert(message, frame);
            alert.addButtonWithTitle(&NSString::from_str("OK"));
            alert.runModal();
            completion_handler.call(());
        }

        #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_javascript_confirm(
            &self,
            _webview: &WKWebView,
            message: &NSString,
            frame: &WKFrameInfo,
            completion_handler: &DynBlock<dyn Fn(Bool)>,
        ) {
            let alert = new_alert(message, frame);
            alert.addButtonWithTitle(&NSString::from_str("OK"));
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
            let accepted = alert.runModal() == NSAlertFirstButtonReturn;
            completion_handler.call((Bool::new(accepted),));
        }

        #[unsafe(method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:))]
        fn run_javascript_prompt(
            &self,
            _webview: &WKWebView,
            prompt: &NSString,
            default_text: Option<&NSString>,
            frame: &WKFrameInfo,
            completion_handler: &DynBlock<dyn Fn(*mut NSString)>,
        ) {
            let alert = new_alert(prompt, frame);
            let empty_text = NSString::from_str("");
            let text_field = NSTextField::textFieldWithString(
                default_text.unwrap_or(&empty_text),
                MainThreadMarker::new().expect("WKUIDelegate runs on the main thread"),
            );
            alert.setAccessoryView(Some(&text_field));
            alert.addButtonWithTitle(&NSString::from_str("OK"));
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));

            if alert.runModal() == NSAlertFirstButtonReturn {
                let value = text_field.stringValue();
                completion_handler.call((Retained::as_ptr(&value).cast_mut(),));
            } else {
                completion_handler.call((ptr::null_mut(),));
            }
        }
    }

    impl RabUIDelegate {
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forwarding_target_for_selector(&self, _selector: Sel) -> Option<&AnyObject> {
            self.ivars()
                .original
                .as_deref()
                .map(AsRef::<AnyObject>::as_ref)
        }
    }
);

impl RabUIDelegate {
    fn new(
        mtm: MainThreadMarker,
        original: Option<Retained<ProtocolObject<dyn WKUIDelegate>>>,
    ) -> Retained<Self> {
        let delegate = mtm
            .alloc::<Self>()
            .set_ivars(RabUIDelegateIvars { original });
        unsafe { msg_send![super(delegate), init] }
    }
}

fn new_alert(message: &NSString, frame: &WKFrameInfo) -> Retained<NSAlert> {
    let mtm = MainThreadMarker::new().expect("WKUIDelegate runs on the main thread");
    let alert = NSAlert::new(mtm);
    // SAFETY: WebKit supplies a valid frame and security origin for the
    // duration of this delegate callback.
    let origin = unsafe { frame.securityOrigin() };
    let host = unsafe { origin.host() }.to_string();
    let title = if host.is_empty() {
        NSString::from_str("This page says")
    } else {
        NSString::from_str(&format!("{host} says"))
    };
    alert.setMessageText(&title);
    alert.setInformativeText(message);
    alert
}

pub fn install_js_dialog_delegate(webview: &wry::WebView) -> Option<Retained<RabUIDelegate>> {
    let native_webview = webview.webview();
    // SAFETY: `native_webview` is a valid WKWebView returned by wry.
    let original = unsafe { native_webview.UIDelegate() };
    let delegate = RabUIDelegate::new(
        MainThreadMarker::new().expect("WKWebView creation must run on the main thread"),
        original,
    );
    // SAFETY: RabUIDelegate implements WKUIDelegate and is retained by the caller.
    unsafe {
        native_webview.setUIDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    }
    Some(delegate)
}
