//! Native GPU view below WKWebView. Tauri's root NSView includes the title bar;
//! the renderer must instead use the exact content rectangle the HTML measures.
use bevy::window::{RawHandleWrapper, WindowWrapper};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSColor, NSView, NSWindowOrderingMode};
use raw_window_handle::{AppKitWindowHandle, RawWindowHandle};
use std::ptr::NonNull;

pub struct NativeSurface {
    view: Retained<NSView>,
    webview: Retained<NSView>,
    parent: Retained<NSView>,
}
impl NativeSurface {
    /// Called only from Tauri's supported with_webview callback on the OS main thread.
    pub unsafe fn new(
        window: &tauri::WebviewWindow,
        webview: *mut std::ffi::c_void,
    ) -> Result<(Self, RawHandleWrapper), Box<dyn std::error::Error>> {
        let mtm = MainThreadMarker::new().ok_or("native surface requires OS main thread")?;
        let webview =
            unsafe { Retained::retain(webview.cast::<NSView>()) }.ok_or("missing WKWebView")?;
        let native_window = webview.window().ok_or("WKWebView has no window")?;
        // Only the webview overlay needs transparency. The OS window is fully
        // backed by the GPU/content background; a clear NSWindow also makes its
        // standard title bar show the desktop through it.
        native_window.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        native_window.setOpaque(true);
        native_window.setTitlebarAppearsTransparent(false);
        native_window.setAlphaValue(1.0);
        let parent = unsafe { webview.superview() }.ok_or("WKWebView has no parent")?;
        let view = NSView::initWithFrame(NSView::alloc(mtm), content_frame(&webview, &parent));
        // Both adapter selection and the final Bevy surface see this same
        // host-owned layer. Otherwise raw-window-metal creates helper sublayers
        // for each surface, including the temporary adapter-selection surface.
        let metal = objc2_quartz_core::CAMetalLayer::new();
        metal.setContentsScale(native_window.backingScaleFactor());
        view.setLayer(Some(&metal));
        view.setWantsLayer(true);
        parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(&webview));
        let mut raw = RawHandleWrapper::new(&WindowWrapper::new(window.clone()))?;
        // The parent view retains this child for the Tauri window's lifetime;
        // RawHandleWrapper retains that window. Host also retains both views and
        // drains/drops the renderer before removing the GPU view on main thread.
        unsafe {
            raw.set_window_handle(RawWindowHandle::AppKit(AppKitWindowHandle::new(
                NonNull::from(&*view).cast(),
            )));
        }
        Ok((
            Self {
                view,
                webview,
                parent,
            },
            raw,
        ))
    }
    pub fn view(&self) -> &NSView {
        &self.view
    }
    pub fn presentation(&self) -> serde_json::Value {
        let window = self.view.window().unwrap();
        let layer = self.view.layer().and_then(|root| {
            root.clone()
                .downcast::<objc2_quartz_core::CAMetalLayer>()
                .ok()
                .or_else(|| {
                    // Main-thread view hierarchy; we only read and retain the
                    // layer list while AppKit cannot mutate it concurrently.
                    unsafe { root.sublayers() }?
                        .iter()
                        .find_map(|l| l.downcast::<objc2_quartz_core::CAMetalLayer>().ok())
                })
        });
        serde_json::json!({
            "windowOpaque":window.isOpaque(), "windowAlpha":window.alphaValue(),
            "titlebarTransparent":window.titlebarAppearsTransparent(),
            "screenMaximumFps":window.screen().map(|s|s.maximumFramesPerSecond()),
            "metal":layer.map(|l|serde_json::json!({"displaySyncEnabled":l.displaySyncEnabled(),
                "maximumDrawableCount":l.maximumDrawableCount(),"presentsWithTransaction":l.presentsWithTransaction(),
                "contentsScale":l.contentsScale(),"drawableSize":[l.drawableSize().width,l.drawableSize().height]}))
        })
    }
    pub fn pointer(&self, point: objc2_foundation::NSPoint) -> (f32, f32) {
        let local = self.view.convertPoint_fromView(point, None);
        (
            local.x as f32,
            (self.view.bounds().size.height - local.y) as f32,
        )
    }
    pub fn sync(&self) -> (u32, u32) {
        if let (Some(layer), Some(window)) = (self.view.layer(), self.view.window()) {
            let scale = window.backingScaleFactor();
            if layer.contentsScale() != scale {
                layer.setContentsScale(scale);
            }
        }
        let frame = content_frame(&self.webview, &self.parent);
        if self.view.frame() != frame {
            println!("IRON_SURFACE_FRAME {:?} -> {:?}", self.view.frame(), frame);
            self.view.setFrame(frame);
        }
        let pixels = self.view.convertRectToBacking(self.view.bounds());
        (
            pixels.size.width.round().max(0.0) as u32,
            pixels.size.height.round().max(0.0) as u32,
        )
    }
}
impl Drop for NativeSurface {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

/// WKWebView's frame can extend under the title bar while its HTML viewport
/// excludes that obscured area. AppKit's layout rectangle is the common content
/// area, expressed in the sibling views' parent coordinates.
fn content_frame(webview: &NSView, parent: &NSView) -> objc2_foundation::NSRect {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let web = webview.frame();
    let Some(window) = webview.window() else {
        return web;
    };
    let layout = parent.convertRect_fromView(window.contentLayoutRect(), None);
    let x = web.origin.x.max(layout.origin.x);
    let y = web.origin.y.max(layout.origin.y);
    let right = (web.origin.x + web.size.width).min(layout.origin.x + layout.size.width);
    let top = (web.origin.y + web.size.height).min(layout.origin.y + layout.size.height);
    NSRect::new(
        NSPoint::new(x, y),
        NSSize::new((right - x).max(0.0), (top - y).max(0.0)),
    )
}
