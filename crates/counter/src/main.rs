use app::prelude::*;
use io_ring::Ring;
use wayland::{WlPointerButtonState, WlPointerEvent};
use widgets::{BG, Click, Counter, Hover, atlas};
use window_manager::prelude::*;

#[derive(State)]
struct CounterState {
    ring: Ring,
    wm: WindowManager,
    #[lens(skip)]
    handle: WindowHandle<Counter>,
    #[lens(skip)]
    pointer: utils::Point,
}

fn main() {
    let ring = Ring::default();
    let mut wm = WindowManager::new(ring.proxy());
    wm.upload_atlas(&atlas::WIDGETS);

    let handle = wm.spawn_window(
        WindowSettings {
            width: 480,
            height: 240,
            clear_color: BG,
            kind: WindowKind::Xdg {
                title: "counter".into(),
            },
            touch_config: None,
            gesture_config: None,
        },
        Counter::new(),
    );

    let state = CounterState {
        ring,
        wm,
        handle,
        pointer: utils::Point::new(-1.0, -1.0),
    };

    let mut app = app::App::new(state)
        .mount(io_ring::module())
        .mount(window_manager::module())
        .mount(app::Module::new().on(on_pointer));

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}

fn on_pointer(s: &mut CounterState, ev: &WlPointerEvent) {
    let handle = s.handle;
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
        } => {
            s.pointer = utils::Point::new(*surface_x, *surface_y);
            handle.set(Hover(Some(s.pointer)), &mut s.wm);
        }
        WlPointerEvent::Leave { .. } => {
            handle.set(Hover(None), &mut s.wm);
        }
        WlPointerEvent::Button {
            state: WlPointerButtonState::Pressed,
            button,
            ..
        } if *button == 0x110 => {
            handle.set(Click(s.pointer), &mut s.wm);
        }
        _ => {}
    }
}
