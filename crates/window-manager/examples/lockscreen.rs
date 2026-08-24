use std::any::Any;

use app::prelude::*;
use app::{Poll, PrePoll, Start};
use io_ring::Ring;
use wayland::{
    ExtSessionLockManagerV1, ExtSessionLockSurfaceV1, ExtSessionLockSurfaceV1Event,
    ExtSessionLockV1, ExtSessionLockV1Event, Handle, Interface, WlOutput, WlPointerButtonState,
    WlPointerEvent, WlRegistryEvent,
};
use widgets::{BG, Click, Counter, Hover, atlas};
use window_manager::prelude::*;
use window_manager::{Color, Surface};

const PANEL_GREEN: Color = Color::from_rgb8(36, 92, 56); // xdg window: "lock"
const LOCK_RED: Color = Color::from_rgb8(120, 34, 40); // lock surface: "unlock"
const BTN_LEFT: u32 = 0x110;

struct SessionLockSurface {
    lock: Handle<ExtSessionLockV1>,
    lock_surface: Handle<ExtSessionLockSurfaceV1>,
    locked: bool,
}

impl Surface for SessionLockSurface {
    fn ack_configure(&self, serial: u32) {
        self.lock_surface.ack_configure(serial);
    }

    fn destroy(&mut self) {
        if self.locked {
            self.lock.unlock_and_destroy();
        } else {
            self.lock.destroy();
        }
        self.lock_surface.destroy();
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(State)]
struct LockScreen {
    ring: Ring,
    wm: WindowManager,
    #[lens(skip)]
    control: WindowHandle<()>,
    #[lens(skip)]
    manager: Option<Handle<ExtSessionLockManagerV1>>,
    #[lens(skip)]
    output: Option<Handle<WlOutput>>,
    #[lens(skip)]
    lock_window: Option<WindowHandle<()>>,
    /// The counter widget, embedded as a desynced subsurface of the lock
    /// surface once it is configured.
    #[lens(skip)]
    widget: Option<WindowHandle<Counter>>,
    /// Last pointer position in the focused surface's local coordinates.
    #[lens(skip)]
    pointer: ui::Point,
}

fn on_registry(s: &mut LockScreen, ev: &WlRegistryEvent) {
    let WlRegistryEvent::Global {
        sender,
        name,
        interface,
        version,
    } = ev
    else {
        return;
    };
    match interface.as_str() {
        ExtSessionLockManagerV1::NAME => s.manager = Some(sender.bind(*name, *version)),
        WlOutput::NAME if s.output.is_none() => s.output = Some(sender.bind(*name, *version)),
        _ => {}
    }
}

fn on_pointer(s: &mut LockScreen, ev: &WlPointerEvent) {
    let WlPointerEvent::Button {
        state: WlPointerButtonState::Pressed,
        button,
        ..
    } = ev
    else {
        return;
    };
    if *button != BTN_LEFT {
        return;
    }
    let Some(target) = s.wm.current_pointer_window() else {
        return;
    };

    match s.lock_window {
        None if target == s.control.id() => lock(s),
        Some(handle) if target == handle.id() => {
            // Destroying the lock window tears down its subsurface children.
            s.widget = None;
            s.wm.destroy(handle.id());
            s.lock_window = None;
        }
        _ => {}
    }
}

/// Drive the embedded counter widget. Pointer events over the widget's
/// subsurface arrive with widget-surface-local coordinates, so the same
/// Hover/Click translation as the standalone counter app works unchanged.
/// Pointer focus is checked so events landing elsewhere on the lock surface
/// (e.g. the unlock button) are not forwarded to the widget.
fn on_widget_pointer(s: &mut LockScreen, ev: &WlPointerEvent) {
    let Some(handle) = s.widget else {
        return;
    };
    let over_widget = s.wm.current_pointer_window() == Some(handle.id());
    match ev {
        WlPointerEvent::Enter {
            surface_x,
            surface_y,
            ..
        }
        | WlPointerEvent::Motion {
            surface_x,
            surface_y,
            ..
        } if over_widget => {
            s.pointer = ui::Point::new(*surface_x, *surface_y);
            handle.set(Hover(Some(s.pointer)), &mut s.wm);
        }
        WlPointerEvent::Leave { .. } => {
            handle.set(Hover(None), &mut s.wm);
        }
        WlPointerEvent::Button {
            state: WlPointerButtonState::Pressed,
            button,
            ..
        } if over_widget && *button == BTN_LEFT => {
            handle.set(Click(s.pointer), &mut s.wm);
        }
        _ => {}
    }
}

fn lock(s: &mut LockScreen) {
    let (Some(manager), Some(output)) = (s.manager.clone(), s.output.clone()) else {
        return;
    };

    let surface = s.wm.create_surface();
    let lock = manager.lock();
    let lock_surface = lock.get_lock_surface(&surface, &output);

    let handle = s.wm.spawn_window_with(
        0,
        0,
        LOCK_RED,
        (),
        surface,
        Box::new(SessionLockSurface {
            lock,
            lock_surface,
            locked: false,
        }),
    );
    s.lock_window = Some(handle);
}

fn on_configure(s: &mut LockScreen, ev: &ExtSessionLockSurfaceV1Event) {
    let ExtSessionLockSurfaceV1Event::Configure {
        serial,
        width,
        height,
        ..
    } = ev;
    let Some(handle) = s.lock_window else {
        return;
    };

    // The first configure reveals the output size: embed the counter centered
    // on the lock surface as a desynced subsurface. The lock window's surface
    // already exists, so the spawn initializes immediately; later configures
    // leave the widget where it is.
    if s.widget.is_none() {
        const WIDGET_W: u32 = 320;
        const WIDGET_H: u32 = 150;
        s.widget = Some(s.wm.spawn_subsurface(
            handle.id(),
            (width.saturating_sub(WIDGET_W) / 2) as i32,
            (height.saturating_sub(WIDGET_H) / 2) as i32,
            WIDGET_W,
            WIDGET_H,
            BG,
            Counter::new(),
        ));
    }

    s.wm.configure(handle.id(), *serial, *width, *height);
}

fn on_lock(s: &mut LockScreen, ev: &ExtSessionLockV1Event) {
    match ev {
        ExtSessionLockV1Event::Locked { .. } => {
            if let Some(handle) = s.lock_window {
                if let Some(role) = s.wm.surface_mut::<SessionLockSurface>(handle.id()) {
                    role.locked = true;
                }
            }
        }
        ExtSessionLockV1Event::Finished { .. } => {
            if let Some(handle) = s.lock_window.take() {
                // Destroying the lock window tears down its subsurface children.
                s.widget = None;
                s.wm.destroy(handle.id());
            }
        }
    }
}

fn main() {
    let ring = Ring::default();
    let mut wm = WindowManager::new(ring.proxy());
    wm.upload_atlas(&atlas::WIDGETS);

    let control = wm.spawn_window(
        WindowSettings {
            width: 400,
            height: 300,
            clear_color: PANEL_GREEN,
            kind: WindowKind::Xdg {
                title: "lock control — click to lock".into(),
            },
            touch_config: None,
            gesture_config: None,
        },
        (),
    );

    let state = LockScreen {
        ring,
        wm,
        control,
        manager: None,
        output: None,
        lock_window: None,
        widget: None,
        pointer: ui::Point::new(-1.0, -1.0),
    };

    let mut app = App::new(state)
        .mount(io_ring::module())
        .mount(window_manager::module())
        .mount(
            Module::new()
                .on(on_registry)
                .on(on_pointer)
                .on(on_widget_pointer)
                .on(on_configure)
                .on(on_lock),
        );

    app.dispatch(&Start);
    loop {
        app.dispatch(&PrePoll);
        app.dispatch(&Poll);
    }
}
