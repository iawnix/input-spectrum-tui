use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use evdev::{Device, EventType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalInputEvent {
    Key { code: u16 },
    Button { code: u16 },
    Move { code: u16, value: i32 },
    Wheel { code: u16, value: i32 },
}

#[derive(Debug)]
pub struct GlobalInput {
    receiver: Receiver<GlobalInputEvent>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl GlobalInput {
    pub fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        for path in input_event_paths().unwrap_or_default() {
            let Ok(mut device) = Device::open(&path) else {
                continue;
            };

            let sender = sender.clone();
            let stop = Arc::clone(&stop);
            threads.push(thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match device.fetch_events() {
                        Ok(events) => {
                            for event in events {
                                if let Some(mapped) =
                                    map_event(event.event_type(), event.code(), event.value())
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

        Self {
            receiver,
            stop,
            threads,
        }
    }

    pub fn drain(&self) -> impl Iterator<Item = GlobalInputEvent> + '_ {
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

fn map_event(event_type: EventType, code: u16, value: i32) -> Option<GlobalInputEvent> {
    match event_type {
        EventType::KEY if value == 1 || value == 2 => {
            if is_mouse_button(code) {
                Some(GlobalInputEvent::Button { code })
            } else {
                Some(GlobalInputEvent::Key { code })
            }
        }
        EventType::RELATIVE if value != 0 => {
            if is_wheel_axis(code) {
                Some(GlobalInputEvent::Wheel { code, value })
            } else {
                Some(GlobalInputEvent::Move { code, value })
            }
        }
        _ => None,
    }
}

fn is_mouse_button(code: u16) -> bool {
    (0x110..=0x117).contains(&code)
}

fn is_wheel_axis(code: u16) -> bool {
    matches!(code, 0x08 | 0x09)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_key_press_and_repeat() {
        assert_eq!(
            map_event(EventType::KEY, 30, 1),
            Some(GlobalInputEvent::Key { code: 30 })
        );
        assert_eq!(
            map_event(EventType::KEY, 30, 2),
            Some(GlobalInputEvent::Key { code: 30 })
        );
    }

    #[test]
    fn ignores_key_release() {
        assert_eq!(map_event(EventType::KEY, 30, 0), None);
    }

    #[test]
    fn maps_mouse_button_and_motion() {
        assert_eq!(
            map_event(EventType::KEY, 0x110, 1),
            Some(GlobalInputEvent::Button { code: 0x110 })
        );
        assert_eq!(
            map_event(EventType::RELATIVE, 0, 4),
            Some(GlobalInputEvent::Move { code: 0, value: 4 })
        );
    }

    #[test]
    fn maps_wheel_axes() {
        assert_eq!(
            map_event(EventType::RELATIVE, 0x08, -1),
            Some(GlobalInputEvent::Wheel {
                code: 0x08,
                value: -1
            })
        );
    }
}
