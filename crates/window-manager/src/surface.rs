use std::any::Any;
use wayland::{Handle, WlSubsurface, XdgSurface, XdgToplevel, ZwlrLayerSurfaceV1};

/// The shell role of a [`Window`](crate::WindowManager). Object-safe so it can
/// live as `Box<dyn Surface>`; deliberately small.
pub trait Surface {
    /// Ack a role `configure` by serial (layer-shell / xdg / session-lock all
    /// have their own `ack_configure`). Called from the generic `configure`
    /// drive point.
    fn ack_configure(&self, serial: u32);

    /// Tear down the role object(s). The window's `wl_surface` is destroyed by
    /// `Window::destroy`, not here.
    fn destroy(&mut self);

    /// Downcast hook: lets app code reach role state it does not own via
    /// `WindowManager::surface_mut::<S>` (e.g. flipping session-lock `locked`).
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// `wlr-layer-shell` role.
pub struct LayerShellSurface {
    pub layer_surface: Handle<ZwlrLayerSurfaceV1>,
}

impl Surface for LayerShellSurface {
    fn ack_configure(&self, serial: u32) {
        self.layer_surface.ack_configure(serial);
    }

    fn destroy(&mut self) {
        self.layer_surface.destroy();
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// `xdg-shell` toplevel role.
pub struct XdgShellSurface {
    pub xdg_surface: Handle<XdgSurface>,
    pub toplevel: Handle<XdgToplevel>,
}

impl Surface for XdgShellSurface {
    fn ack_configure(&self, serial: u32) {
        self.xdg_surface.ack_configure(serial);
    }

    fn destroy(&mut self) {
        self.toplevel.destroy();
        self.xdg_surface.destroy();
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// `wl_subcompositor` role: a window whose `wl_surface` is a subsurface of
/// another window's surface. A subsurface is not a shell role — it needs no
/// `ack_configure` — this exists only to satisfy the [`Surface`] seam and own
/// the `wl_subsurface` object's lifetime.
pub struct SubsurfaceRole {
    pub subsurface: Handle<WlSubsurface>,
}

impl Surface for SubsurfaceRole {
    fn ack_configure(&self, _serial: u32) {}

    fn destroy(&mut self) {
        self.subsurface.destroy();
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
