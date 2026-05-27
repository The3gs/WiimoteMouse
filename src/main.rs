use std::ffi::{CStr, CString};

#[repr(C)]
enum Led {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct EventKey {
    code: libc::c_uint,
    state: libc::c_uint,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct EventAbs {
    x: libc::c_int,
    y: libc::c_int,
    z: libc::c_int,
}

#[repr(C)]
union EventUnion {
    key: EventKey,
    abs: [EventAbs; 4],
    reserved: [u8; 128],
}

#[repr(C)]
struct EventSys {
    time: libc::timeval,
    type_: libc::c_uint,
    v: EventUnion,
}

#[non_exhaustive]
#[derive(Debug)]
enum Event {
    Other,
    Accel(EventAbs),
    Key(EventKey),
    Ir([EventAbs; 4]),
}

mod monitor;

use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode,
    UinputAbsSetup, uinput::VirtualDevice,
};
use monitor::Monitor;

unsafe extern "C" {
    fn xwii_iface_new(dev: *mut *const libc::c_void, path: *const libc::c_char) -> libc::c_int;

    fn xwii_iface_ref(dev: *const libc::c_void);

    fn xwii_iface_unref(dev: *const libc::c_void);

    fn xwii_iface_get_fd(dev: *const libc::c_void) -> libc::c_int;

    fn xwii_iface_open(dev: *const libc::c_void, ifaces: libc::c_uint) -> libc::c_int;

    fn xwii_iface_close(dev: *const libc::c_void, ifaces: libc::c_uint);

    fn xwii_iface_opened(dev: *const libc::c_void) -> libc::c_uint;

    fn xwii_iface_available(dev: *const libc::c_void) -> libc::c_uint;

    fn xwii_iface_dispatch(
        dev: *const libc::c_void,
        ev: *mut EventSys,
        size: libc::size_t,
    ) -> libc::c_int;

    fn xwii_iface_rumble(dev: *const libc::c_void, on: bool) -> libc::c_int;

    fn xwii_iface_get_led(
        dev: *const libc::c_void,
        led: libc::c_uint,
        state: *mut bool,
    ) -> libc::c_int;

    fn xwii_iface_set_led(dev: *const libc::c_void, led: libc::c_uint, state: bool) -> libc::c_int;

    fn xwii_iface_get_battery(dev: *const libc::c_void, capacity: *mut u8) -> libc::c_int;
}

struct Device {
    ptr: *const libc::c_void,
    fd: libc::c_int,
}

impl Device {
    fn new(path: &str) -> Result<Self, ()> {
        let mut ptr: *const libc::c_void = std::ptr::null();
        let c_string = CString::new(path).unwrap();
        let err = unsafe { xwii_iface_new(&mut ptr, c_string.into_raw()) };
        if err != 0 {
            return Err(());
        }

        let fd = unsafe { xwii_iface_get_fd(ptr) };
        Ok(Self { ptr, fd })
    }

    fn get_battery(&self) -> u8 {
        let mut capacity = 0u8;
        unsafe {
            xwii_iface_get_battery(self.ptr, &mut capacity);
        }
        return capacity;
    }

    fn set_led(&self, led_index: u32, enabled: bool) -> Result<(), ()> {
        if unsafe { xwii_iface_set_led(self.ptr, led_index + 1, enabled) } != 0 {
            Err(())
        } else {
            Ok(())
        }
    }

    fn wait_for_event(&self) -> Result<Event, ()> {
        let mut events = [libc::epoll_event { events: 0, u64: 0 }; 32];
        let event_count = unsafe {
            libc::epoll_wait(
                self.fd,
                (&mut events[..]).as_mut_ptr(),
                events.len() as i32,
                -1,
            )
        };
        if (0..event_count).any(|i| events[i as usize].events as i32 & libc::EPOLLIN != 0) {
            let mut event: EventSys = EventSys {
                time: libc::timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                },
                type_: 0,
                v: EventUnion {
                    key: EventKey { code: 0, state: 0 },
                },
            };
            let res = unsafe {
                xwii_iface_dispatch(
                    self.ptr,
                    (&mut event) as *mut EventSys,
                    std::mem::size_of::<EventSys>(),
                )
            };
            let event = match event.type_ {
                0 => Event::Key(unsafe { event.v.key }),
                1 => Event::Accel(unsafe { event.v.abs }[0]),
                2 => Event::Ir(unsafe { event.v.abs }),
                _ => Event::Other,
            };
            if res == 0 { Ok(event) } else { Err(()) }
        } else {
            Err(())
        }
    }
}

impl Clone for Device {
    fn clone(&self) -> Self {
        unsafe {
            xwii_iface_ref(self.ptr);
        }
        Self {
            ptr: self.ptr,
            fd: self.fd,
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            xwii_iface_unref(self.ptr);
        }
    }
}

enum PointerState {
    Calibrate,
    Pointing,
    Paused,
}

fn main() {
    let mut mouse_buttons = AttributeSet::<KeyCode>::new();
    mouse_buttons.insert(KeyCode::BTN_LEFT);
    mouse_buttons.insert(KeyCode::BTN_RIGHT);
    let mut mouse = VirtualDevice::builder()
        .unwrap()
        .name("wii-mouse")
        .with_keys(&mouse_buttons)
        .unwrap()
        .with_relative_axes(&AttributeSet::from_iter([
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
        ]))
        .unwrap()
        .build()
        .unwrap();

    let mut pos = (0, 0);
    let mut state = PointerState::Calibrate;

    let monitor = Monitor::new(false, false);
    for wiimote_path in monitor {
        println!("path: {wiimote_path:?}");
        let device = Device::new(&wiimote_path).unwrap();
        println!("battery: {}%", device.get_battery());
        if device.set_led(0, true).is_err() {
            println!("Failed to set LED. Try running as sudo.")
        };

        assert!(unsafe { xwii_iface_open(device.ptr, 5) } == 0);

        loop {
            let Ok(event) = device.wait_for_event() else {
                continue;
            };
            //println!("{event:?}");
            match state {
                PointerState::Calibrate => {
                    println!("Callibration not implemented, Pausing in stead.");
                    state = PointerState::Paused;
                }
                PointerState::Pointing => match event {
                    Event::Key(EventKey { code, state: st }) => {
                        if st == 1 || [4, 5].contains(&code) {
                            match code {
                                8 => break,
                                4 => {
                                    mouse
                                        .emit(&[InputEvent::new_now(
                                            EventType::KEY.0,
                                            KeyCode::BTN_LEFT.0,
                                            st as i32,
                                        )])
                                        .unwrap();
                                }
                                5 => {
                                    mouse
                                        .emit(&[InputEvent::new_now(
                                            EventType::KEY.0,
                                            KeyCode::BTN_RIGHT.0,
                                            st as i32,
                                        )])
                                        .unwrap();
                                }
                                9 => {
                                    state = PointerState::Calibrate;
                                }
                                10 => {
                                    state = PointerState::Paused;
                                }
                                _ => (),
                            }
                        }
                    }
                    Event::Ir([pt0, ..]) => {
                        let (dx, dy) = (pt0.x - pos.0, pt0.y - pos.1);
                        if dx.abs() < 100 && dy.abs() < 100 && (dx.abs() > 3 || dy.abs() > 3) {
                            mouse
                                .emit(&[
                                    InputEvent::new_now(
                                        EventType::RELATIVE.0,
                                        RelativeAxisCode::REL_X.0,
                                        -dx * 2,
                                    ),
                                    InputEvent::new_now(
                                        EventType::RELATIVE.0,
                                        RelativeAxisCode::REL_Y.0,
                                        dy * 2,
                                    ),
                                ])
                                .unwrap();
                            print!(
                                "\rD ({}, {}) A ({}, {})                        ",
                                pt0.x - pos.0,
                                pt0.y - pos.1,
                                pt0.x,
                                pt0.y
                            );
                        }
                        pos = (pt0.x, pt0.y);
                    }
                    _ => (),
                },
                PointerState::Paused => {
                    let Event::Key(key) = event else { continue };

                    if key.code == 10 && key.state == 1 {
                        state = PointerState::Pointing;
                        pos = (0, 0);
                        continue;
                    }
                    if key.code == 9 && key.state == 1 {
                        state = PointerState::Calibrate;
                    }
                }
            }
            if let Event::Key(k) = event {
                println!("{}: {}", k.code, ["Released", "Pressed"][k.state as usize]);
                if k.code == 8 {
                    break;
                }
                if k.code == 4 {
                    mouse
                        .emit(&[InputEvent::new_now(
                            EventType::KEY.0,
                            KeyCode::BTN_LEFT.0,
                            k.state as i32,
                        )])
                        .unwrap();
                }
            }
        }
    }
}
