use std::ffi::{c_char, c_void};

#[link(name = "objc")]
unsafe extern "C" {
  fn objc_getClass(name: *const c_char) -> *mut c_void;
  fn objc_msgSend();
  fn sel_registerName(name: *const c_char) -> *const c_void;
}

unsafe fn objc_send_id(receiver: *mut c_void, selector: &'static [u8]) -> *mut c_void {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector)
}

unsafe fn objc_send_bool(receiver: *mut c_void, selector: &'static [u8], value: bool) {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void, bool) =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector, value);
}

unsafe fn objc_send_id_arg(receiver: *mut c_void, selector: &'static [u8], value: *mut c_void) {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector, value);
}

unsafe fn objc_send_f64(receiver: *mut c_void, selector: &'static [u8], value: f64) {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector, value);
}

pub(super) fn apply_native_window_corner_radius(
  window: &tauri::WebviewWindow,
) -> Result<(), String> {
  let native_window = window.ns_window().map_err(|error| error.to_string())?;
  let (clear_color, content_view) = unsafe {
    let ns_color = objc_getClass(c"NSColor".as_ptr());
    if ns_color.is_null() {
      return Err("无法获取 macOS 原生颜色类".to_string());
    }
    (
      objc_send_id(ns_color, b"clearColor\0"),
      objc_send_id(native_window, b"contentView\0"),
    )
  };
  if clear_color.is_null() {
    return Err("无法获取 macOS 透明背景色".to_string());
  }
  if content_view.is_null() {
    return Err("无法获取 macOS 窗口内容视图".to_string());
  }

  unsafe {
    objc_send_bool(native_window, b"setOpaque:\0", false);
    objc_send_id_arg(native_window, b"setBackgroundColor:\0", clear_color);
    objc_send_bool(content_view, b"setWantsLayer:\0", true);
    let layer = objc_send_id(content_view, b"layer\0");
    if layer.is_null() {
      return Err("无法获取 macOS 窗口内容图层".to_string());
    }
    objc_send_f64(layer, b"setCornerRadius:\0", 16.0);
    objc_send_bool(layer, b"setMasksToBounds:\0", true);
  }
  Ok(())
}
