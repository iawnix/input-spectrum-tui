use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    stops: Vec<Arc<AtomicBool>>,
    threads: Vec<JoinHandle<()>>,
}

impl GlobalInput {
    pub fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut threads = Vec::new();
        let mut stops = Vec::new();
        let debug = DebugLogInner::from_env();
        let backend = BackendPreference::from_env();

        debug.log(format!("global input start backend={}", backend.name()));

        match backend {
            BackendPreference::Auto => {
                if let Some(backend) = start_x11_record_backend(
                    sender.clone(),
                    Arc::clone(&debug),
                    Duration::from_millis(700),
                ) {
                    stops.push(backend.stop);
                    threads.push(backend.thread);
                } else {
                    let evdev = start_evdev_keyboard_backend(sender, Arc::clone(&debug));
                    stops.push(evdev.stop);
                    threads.extend(evdev.threads);
                }
            }
            BackendPreference::X11 => {
                if let Some(backend) =
                    start_x11_record_backend(sender, Arc::clone(&debug), Duration::from_secs(2))
                {
                    stops.push(backend.stop);
                    threads.push(backend.thread);
                }
            }
            BackendPreference::Evdev => {
                let evdev = start_evdev_keyboard_backend(sender, Arc::clone(&debug));
                stops.push(evdev.stop);
                threads.extend(evdev.threads);
            }
            BackendPreference::None => {}
        }

        Self {
            receiver,
            stops,
            threads,
        }
    }

    pub fn drain(&self) -> impl Iterator<Item = GlobalKeyEvent> + '_ {
        self.receiver.try_iter()
    }
}

impl Drop for GlobalInput {
    fn drop(&mut self) {
        for stop in &self.stops {
            stop.store(true, Ordering::Relaxed);
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendPreference {
    Auto,
    X11,
    Evdev,
    None,
}

impl BackendPreference {
    fn from_env() -> Self {
        match std::env::var("INPUTSPECTRUM_BACKEND") {
            Ok(value) if value.eq_ignore_ascii_case("x11") => Self::X11,
            Ok(value) if value.eq_ignore_ascii_case("evdev") => Self::Evdev,
            Ok(value) if value.eq_ignore_ascii_case("none") => Self::None,
            _ => Self::Auto,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::X11 => "x11",
            Self::Evdev => "evdev",
            Self::None => "none",
        }
    }
}

type DebugLog = Arc<DebugLogInner>;

#[derive(Debug)]
struct DebugLogInner {
    file: Option<Mutex<File>>,
}

impl DebugLogInner {
    fn from_env() -> DebugLog {
        let file = std::env::var_os("INPUTSPECTRUM_DEBUG_LOG").and_then(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(Mutex::new)
        });
        Arc::new(Self { file })
    }

    fn log(&self, message: impl AsRef<str>) {
        let Some(file) = &self.file else {
            return;
        };
        let Ok(mut file) = file.lock() else {
            return;
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let _ = writeln!(file, "[{timestamp}] {}", message.as_ref());
    }
}

struct InputBackend {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

struct EvdevBackend {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

fn start_x11_record_backend(
    sender: Sender<GlobalKeyEvent>,
    debug: DebugLog,
    timeout: Duration,
) -> Option<InputBackend> {
    if std::env::var_os("DISPLAY").is_none() {
        debug.log("x11 skipped: DISPLAY is not set");
        return None;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_debug = Arc::clone(&debug);
    let (ready_sender, ready_receiver) = mpsc::channel();
    let thread = thread::spawn(move || unsafe {
        let result = start_x11_record_thread(sender, Arc::clone(&thread_stop), Arc::clone(&thread_debug));
        let ready = result.is_some();
        let _ = ready_sender.send(ready);
        if let Some(backend) = result {
            x11_record_loop(backend, thread_stop);
        }
    });

    match ready_receiver.recv_timeout(timeout) {
        Ok(true) => {
            debug.log("x11 backend ready");
            Some(InputBackend { stop, thread })
        }
        Ok(false) => {
            debug.log("x11 backend failed; falling back if auto");
            stop.store(true, Ordering::Relaxed);
            let _ = thread.join();
            None
        }
        Err(_) => {
            debug.log(format!("x11 backend timed out after {} ms", timeout.as_millis()));
            stop.store(true, Ordering::Relaxed);
            None
        }
    }
}

unsafe fn start_x11_record_thread(
    sender: Sender<GlobalKeyEvent>,
    stop: Arc<AtomicBool>,
    debug: DebugLog,
) -> Option<X11RecordBackend> {
    if stop.load(Ordering::Relaxed) {
        debug.log("x11 init cancelled before start");
        return None;
    }

    unsafe {
        debug.log("x11 init: XInitThreads");
        let _ = xlib::XInitThreads();
        debug.log("x11 init: opening displays");
        let dpy_control = xlib::XOpenDisplay(null());
        let dpy_data = xlib::XOpenDisplay(null());
        if dpy_control.is_null() || dpy_data.is_null() {
            close_display(dpy_control);
            close_display(dpy_data);
            debug.log("x11 init failed: cannot open display");
            return None;
        }

        debug.log("x11 init: installing x error trap");
        let error_trap = X11ErrorTrap::install();

        let extension_name = CString::new("RECORD").expect("static string has no nul byte");
        debug.log("x11 init: XInitExtension(RECORD)");
        if xlib::XInitExtension(dpy_control, extension_name.as_ptr()).is_null() {
            close_display(dpy_control);
            close_display(dpy_data);
            debug.log("x11 init failed: RECORD extension missing");
            return None;
        }

        let mut major: c_int = 0;
        let mut minor: c_int = 0;
        debug.log("x11 init: XRecordQueryVersion");
        if xrecord::XRecordQueryVersion(dpy_control, &mut major, &mut minor) == 0 {
            close_display(dpy_control);
            close_display(dpy_data);
            debug.log("x11 init failed: XRecordQueryVersion failed");
            return None;
        }
        debug.log(format!("x11 init: RECORD version {major}.{minor}"));

        debug.log("x11 init: XRecordAllocRange");
        let range_ptr = xrecord::XRecordAllocRange();
        if range_ptr.is_null() {
            close_display(dpy_control);
            close_display(dpy_data);
            debug.log("x11 init failed: XRecordAllocRange failed");
            return None;
        }
        (*range_ptr).device_events.first = xlib::KeyPress as u8;
        (*range_ptr).device_events.last = xlib::KeyPress as u8;

        let mut clients = xrecord::XRecordAllClients;
        let mut range_for_context = range_ptr;
        debug.log("x11 init: XRecordCreateContext");
        let context = xrecord::XRecordCreateContext(
            dpy_control,
            0,
            &mut clients,
            1,
            &mut range_for_context,
            1,
        );
        debug.log("x11 init: XSync(control) after create context");
        xlib::XSync(dpy_control, 0);
        if context == 0 {
            xlib::XFree(range_ptr.cast());
            close_display(dpy_control);
            close_display(dpy_data);
            debug.log("x11 init failed: XRecordCreateContext returned 0");
            return None;
        }
        if error_trap.caught_error() {
            xrecord::XRecordFreeContext(dpy_control, context);
            xlib::XFree(range_ptr.cast());
            close_display(dpy_control);
            close_display(dpy_data);
            debug.log("x11 init failed: X error during create context");
            return None;
        }

        let mut callback_state = Box::new(X11CallbackState { sender });
        let callback_state_ptr = (&mut *callback_state as *mut X11CallbackState).cast::<c_char>();
        debug.log("x11 init: XRecordEnableContextAsync");
        let enable_result = xrecord::XRecordEnableContextAsync(
            dpy_data,
            context,
            Some(x11_record_callback),
            callback_state_ptr,
        );
        debug.log("x11 init: XSync(data) after enable context");
        xlib::XSync(dpy_data, 0);
        debug.log("x11 init: XSync(control) after enable context");
        xlib::XSync(dpy_control, 0);
        if enable_result == 0 || error_trap.caught_error() {
            xrecord::XRecordDisableContext(dpy_control, context);
            xrecord::XRecordFreeContext(dpy_control, context);
            xlib::XFree(range_ptr.cast());
            close_display(dpy_control);
            close_display(dpy_data);
            debug.log(format!(
                "x11 init failed: enable_result={enable_result}, trapped_error={}",
                error_trap.caught_error()
            ));
            return None;
        }

        drop(error_trap);

        debug.log("x11 init: ready");
        Some(X11RecordBackend {
            dpy_control,
            dpy_data,
            range_ptr,
            context,
            callback_state,
            debug,
        })
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
    debug: DebugLog,
}

unsafe impl Send for X11RecordBackend {}

fn x11_record_loop(backend: X11RecordBackend, stop: Arc<AtomicBool>) {
    unsafe {
        backend.debug.log("x11 loop: started");
        while !stop.load(Ordering::Relaxed) {
            xrecord::XRecordProcessReplies(backend.dpy_data);
            thread::sleep(Duration::from_millis(2));
        }

        backend.debug.log("x11 loop: stopping");
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
    debug: DebugLog,
) -> EvdevBackend {
    let stop = Arc::new(AtomicBool::new(false));
    let mut threads = Vec::new();
    let paths = input_event_paths().unwrap_or_default();
    debug.log(format!("evdev init: {} candidate devices", paths.len()));
    for path in paths {
        let Ok(mut device) = Device::open(&path) else {
            debug.log(format!("evdev skip: cannot open {}", path.display()));
            continue;
        };
        if device.set_nonblocking(true).is_err() {
            debug.log(format!("evdev skip: cannot set nonblocking {}", path.display()));
            continue;
        }

        let sender = sender.clone();
        let stop = Arc::clone(&stop);
        let debug = Arc::clone(&debug);
        debug.log(format!("evdev init: listening {}", path.display()));
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
                    Err(error) => {
                        debug.log(format!("evdev thread stopped: {error}"));
                        return;
                    }
                }
            }
        }));
    }
    debug.log(format!("evdev init: {} listener threads", threads.len()));
    EvdevBackend { stop, threads }
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
