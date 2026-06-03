use std::ffi::CString;
use std::fs;
use std::io;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use evdev::{Device, EventType};
use x11::{xlib, xrecord};

static X11_ERROR_TRAPPED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalKeyEvent {
    pub code: u16,
}

#[derive(Debug)]
pub struct GlobalInput {
    receiver: Receiver<GlobalKeyEvent>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl GlobalInput {
    pub fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        if let Some(thread) = start_x11_record_backend(sender.clone(), Arc::clone(&stop)) {
            threads.push(thread);
        } else {
            threads.extend(start_evdev_keyboard_backend(sender, Arc::clone(&stop)));
        }

        Self {
            receiver,
            stop,
            threads,
        }
    }

    pub fn drain(&self) -> impl Iterator<Item = GlobalKeyEvent> + '_ {
        self.receiver.try_iter()
    }
}

impl Drop for GlobalInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn start_x11_record_backend(
    sender: Sender<GlobalKeyEvent>,
    stop: Arc<AtomicBool>,
) -> Option<JoinHandle<()>> {
    if std::env::var_os("DISPLAY").is_none() {
        return None;
    }

    unsafe {
        let _ = xlib::XInitThreads();
        let dpy_control = xlib::XOpenDisplay(null());
        let dpy_data = xlib::XOpenDisplay(null());
        if dpy_control.is_null() || dpy_data.is_null() {
            close_display(dpy_control);
            close_display(dpy_data);
            return None;
        }

        let error_trap = X11ErrorTrap::install();

        let extension_name = CString::new("RECORD").expect("static string has no nul byte");
        if xlib::XInitExtension(dpy_control, extension_name.as_ptr()).is_null() {
            close_display(dpy_control);
            close_display(dpy_data);
            return None;
        }

        let mut major: c_int = 0;
        let mut minor: c_int = 0;
        if xrecord::XRecordQueryVersion(dpy_control, &mut major, &mut minor) == 0 {
            close_display(dpy_control);
            close_display(dpy_data);
            return None;
        }

        let range_ptr = xrecord::XRecordAllocRange();
        if range_ptr.is_null() {
            close_display(dpy_control);
            close_display(dpy_data);
            return None;
        }
        (*range_ptr).device_events.first = xlib::KeyPress as u8;
        (*range_ptr).device_events.last = xlib::KeyPress as u8;

        let mut clients = xrecord::XRecordAllClients;
        let mut range_for_context = range_ptr;
        let context = xrecord::XRecordCreateContext(
            dpy_control,
            0,
            &mut clients,
            1,
            &mut range_for_context,
            1,
        );
        xlib::XSync(dpy_control, 0);
        if context == 0 {
            xlib::XFree(range_ptr.cast());
            close_display(dpy_control);
            close_display(dpy_data);
            return None;
        }
        if error_trap.caught_error() {
            xrecord::XRecordFreeContext(dpy_control, context);
            xlib::XFree(range_ptr.cast());
            close_display(dpy_control);
            close_display(dpy_data);
            return None;
        }

        let mut callback_state = Box::new(X11CallbackState { sender });
        let callback_state_ptr = (&mut *callback_state as *mut X11CallbackState).cast::<c_char>();
        let enable_result = xrecord::XRecordEnableContextAsync(
            dpy_data,
            context,
            Some(x11_record_callback),
            callback_state_ptr,
        );
        xlib::XSync(dpy_data, 0);
        xlib::XSync(dpy_control, 0);
        if enable_result == 0 || error_trap.caught_error() {
            xrecord::XRecordDisableContext(dpy_control, context);
            xrecord::XRecordFreeContext(dpy_control, context);
            xlib::XFree(range_ptr.cast());
            close_display(dpy_control);
            close_display(dpy_data);
            return None;
        }

        drop(error_trap);

        let backend = X11RecordBackend {
            dpy_control,
            dpy_data,
            range_ptr,
            context,
            callback_state,
        };
        Some(thread::spawn(move || x11_record_loop(backend, stop)))
    }
}

struct X11ErrorTrap {
    previous: Option<unsafe extern "C" fn(*mut xlib::Display, *mut xlib::XErrorEvent) -> c_int>,
}

impl X11ErrorTrap {
    unsafe fn install() -> Self {
        X11_ERROR_TRAPPED.store(false, Ordering::SeqCst);
        let previous = xlib::XSetErrorHandler(Some(x11_error_handler));
        Self { previous }
    }

    fn caught_error(&self) -> bool {
        X11_ERROR_TRAPPED.load(Ordering::SeqCst)
    }
}

impl Drop for X11ErrorTrap {
    fn drop(&mut self) {
        unsafe {
            xlib::XSetErrorHandler(self.previous);
        }
    }
}

unsafe extern "C" fn x11_error_handler(
    _display: *mut xlib::Display,
    _event: *mut xlib::XErrorEvent,
) -> c_int {
    X11_ERROR_TRAPPED.store(true, Ordering::SeqCst);
    0
}

struct X11RecordBackend {
    dpy_control: *mut xlib::Display,
    dpy_data: *mut xlib::Display,
    range_ptr: *mut xrecord::XRecordRange,
    context: xrecord::XRecordContext,
    callback_state: Box<X11CallbackState>,
}

unsafe impl Send for X11RecordBackend {}

fn x11_record_loop(backend: X11RecordBackend, stop: Arc<AtomicBool>) {
    unsafe {
        while !stop.load(Ordering::Relaxed) {
            xrecord::XRecordProcessReplies(backend.dpy_data);
            thread::sleep(Duration::from_millis(2));
        }

        xrecord::XRecordDisableContext(backend.dpy_control, backend.context);
        xrecord::XRecordFreeContext(backend.dpy_control, backend.context);
        xlib::XFree(backend.range_ptr.cast());
        close_display(backend.dpy_control);
        close_display(backend.dpy_data);
        drop(backend.callback_state);
    }
}

fn start_evdev_keyboard_backend(
    sender: Sender<GlobalKeyEvent>,
    stop: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>> {
    let mut threads = Vec::new();
    for path in input_event_paths().unwrap_or_default() {
        let Ok(mut device) = Device::open(&path) else {
            continue;
        };
        if device.set_nonblocking(true).is_err() {
            continue;
        }

        let sender = sender.clone();
        let stop = Arc::clone(&stop);
        threads.push(thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            if let Some(mapped) =
                                map_evdev_key(event.event_type(), event.code(), event.value())
                            {
                                if sender.send(mapped).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) =>
                    {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => return,
                }
            }
        }));
    }
    threads
}

fn input_event_paths() -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir("/dev/input")? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("event"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn map_evdev_key(event_type: EventType, code: u16, value: i32) -> Option<GlobalKeyEvent> {
    match event_type {
        EventType::KEY if (value == 1 || value == 2) && !is_non_keyboard_button(code) => {
            Some(GlobalKeyEvent { code })
        }
        _ => None,
    }
}

fn is_non_keyboard_button(code: u16) -> bool {
    matches!(code, 0x100..=0x15f | 0x220..=0x223 | 0x2c0..=0x2ff)
}

unsafe fn close_display(display: *mut xlib::Display) {
    if !display.is_null() {
        xlib::XCloseDisplay(display);
    }
}

struct X11CallbackState {
    sender: Sender<GlobalKeyEvent>,
}

#[repr(C)]
struct XRecordDatum {
    event_type: u8,
    code: u8,
    _sequence: u16,
}

unsafe extern "C" fn x11_record_callback(
    state: *mut c_char,
    raw_data: *mut xrecord::XRecordInterceptData,
) {
    if raw_data.is_null() {
        return;
    }

    let data = &*raw_data;
    if data.category == xrecord::XRecordFromServer && !data.data.is_null() {
        let event = &*(data.data as *const XRecordDatum);
        if event.event_type as c_int == xlib::KeyPress {
            let callback_state = &mut *(state.cast::<X11CallbackState>());
            let _ = callback_state.sender.send(GlobalKeyEvent {
                code: event.code as u16,
            });
        }
    }

    xrecord::XRecordFreeData(raw_data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_key_press_and_repeat() {
        assert_eq!(
            map_evdev_key(EventType::KEY, 30, 1),
            Some(GlobalKeyEvent { code: 30 })
        );
        assert_eq!(
            map_evdev_key(EventType::KEY, 30, 2),
            Some(GlobalKeyEvent { code: 30 })
        );
    }

    #[test]
    fn ignores_key_release() {
        assert_eq!(map_evdev_key(EventType::KEY, 30, 0), None);
    }

    #[test]
    fn ignores_mouse_buttons_and_motion() {
        assert_eq!(map_evdev_key(EventType::KEY, 0x110, 1), None);
        assert_eq!(map_evdev_key(EventType::KEY, 0x130, 1), None);
        assert_eq!(map_evdev_key(EventType::KEY, 0x2c0, 1), None);
        assert_eq!(map_evdev_key(EventType::RELATIVE, 0, 4), None);
    }
}
