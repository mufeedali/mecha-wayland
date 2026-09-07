use anyhow::{Result, bail};
use gbm::{BufferObjectFlags, Format as GbmFormat, Modifier};
use glow::HasContext;
use khronos_egl as egl;
use std::{
    ffi::c_void,
    os::unix::io::{AsRawFd, OwnedFd},
};

use crate::{Renderer, SurfaceBackend};

// ── EGL_EXT_image_dma_buf_import constants ────────────────────────────────

const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: i32 = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: i32 = 0x3444;
const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

// ── DmaBuf ────────────────────────────────────────────────────────────────

/// Wayland-presentable render target: GBM buffer object imported as EGLImageKHR,
/// bound to an FBO for GL rendering. Color is a write-only RBO by default;
/// [`DmaBuf::allocate_texture`] uses a `TEXTURE_2D` so another renderer can bind it.
///
/// To present: `dup` the `prime_fd` and attach it to a `wl_buffer` via
/// `zwp_linux_dmabuf_v1`. Keep one `RenderableSurface<DmaBuf>` per swap slot;
/// the caller manages buffer lifecycle and swap chain rotation.
pub struct DmaBuf {
    _gbm_bo: gbm::BufferObject<()>,    // must outlive egl_image
    pub prime_fd: OwnedFd,             // DMA-BUF fd to hand to Wayland (dup before sending)
    pub egl_image: *mut c_void,        // EGLImageKHR wrapping the DMA-BUF memory
    pub stride: u32,                   // bytes per row
    pub modifier: u64,                 // DRM format modifier (for Wayland params)
    rbo: Option<glow::Renderbuffer>,   // None when color is a texture
    color_tex: Option<glow::Texture>,  // Some when allocated with allocate_texture
    pub depth_rbo: glow::Renderbuffer, // depth renderbuffer at DEPTH_ATTACHMENT
    pub fbo: glow::Framebuffer,        // GL framebuffer with color at COLOR_ATTACHMENT0
}

// SAFETY: EGLImageKHR pointer and GL handles are only accessed from one thread.
unsafe impl Send for DmaBuf {}

fn allocate(renderer: &Renderer, width: u32, height: u32, color_texture: bool) -> Result<DmaBuf> {
    // Flags-only gbm_bo_create reports INVALID; ask for LINEAR by modifier.
    let gbm_bo = renderer
        ._gbm_device
        .create_buffer_object_with_modifiers2::<()>(
            width,
            height,
            GbmFormat::Argb8888,
            [Modifier::Linear].into_iter(),
            BufferObjectFlags::RENDERING,
        )
        .map_err(|e| anyhow::anyhow!("gbm_bo_create: {e}"))?;

    let modifier = u64::from(gbm_bo.modifier());
    tracing::debug!(modifier = format!("0x{modifier:016x}"), "gbm bo modifier");

    let prime_fd = gbm_bo
        .fd()
        .map_err(|e| anyhow::anyhow!("gbm_bo_get_fd: {e}"))?;
    let stride = gbm_bo.stride();
    let raw_display = renderer.display.as_ptr() as *const c_void;

    let import_attribs: [i32; 17] = [
        egl::WIDTH,
        width as i32,
        egl::HEIGHT,
        height as i32,
        EGL_LINUX_DRM_FOURCC_EXT,
        DRM_FORMAT_ARGB8888 as i32,
        EGL_DMA_BUF_PLANE0_FD_EXT,
        prime_fd.as_raw_fd(),
        EGL_DMA_BUF_PLANE0_OFFSET_EXT,
        0,
        EGL_DMA_BUF_PLANE0_PITCH_EXT,
        stride as i32,
        EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
        (modifier & 0xffff_ffff) as i32,
        EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
        (modifier >> 32) as i32,
        egl::NONE,
    ];

    let egl_image = unsafe {
        (renderer.fn_create_image)(
            raw_display,
            std::ptr::null(),
            EGL_LINUX_DMA_BUF_EXT,
            std::ptr::null(),
            import_attribs.as_ptr(),
        )
    };
    if egl_image.is_null() {
        bail!("eglCreateImageKHR(EGL_LINUX_DMA_BUF_EXT) failed");
    }
    tracing::info!(width, height, "DmaBuf EGLImageKHR created");

    let (rbo, color_tex) = if color_texture {
        let fn_tex_image = renderer
            .fn_tex_image
            .ok_or_else(|| anyhow::anyhow!("glEGLImageTargetTexture2DOES not available"))?;
        let tex = unsafe { renderer.gl.create_texture() }
            .map_err(|e| anyhow::anyhow!("glGenTextures: {e}"))?;
        unsafe {
            renderer.gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            renderer.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            renderer.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            renderer.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            renderer.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            fn_tex_image(glow::TEXTURE_2D, egl_image);
        }
        (None, Some(tex))
    } else {
        let rbo = unsafe { renderer.gl.create_renderbuffer() }
            .map_err(|e| anyhow::anyhow!("glGenRenderbuffers: {e}"))?;
        unsafe {
            renderer.gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rbo));
            (renderer.fn_rbo_image)(glow::RENDERBUFFER, egl_image);
        }
        (Some(rbo), None)
    };

    let fbo = unsafe { renderer.gl.create_framebuffer() }
        .map_err(|e| anyhow::anyhow!("glGenFramebuffers: {e}"))?;
    let depth_rbo = unsafe { renderer.gl.create_renderbuffer() }
        .map_err(|e| anyhow::anyhow!("glGenRenderbuffers (depth): {e}"))?;
    unsafe {
        renderer
            .gl
            .bind_renderbuffer(glow::RENDERBUFFER, Some(depth_rbo));
        renderer.gl.renderbuffer_storage(
            glow::RENDERBUFFER,
            glow::DEPTH_COMPONENT24,
            width as i32,
            height as i32,
        );
        renderer.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        if let Some(tex) = color_tex {
            renderer.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
        } else {
            renderer.gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::RENDERBUFFER,
                rbo,
            );
        }
        renderer.gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_ATTACHMENT,
            glow::RENDERBUFFER,
            Some(depth_rbo),
        );
        let status = renderer.gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if status != glow::FRAMEBUFFER_COMPLETE {
            bail!("FBO incomplete: 0x{:x}", status);
        }
    }
    tracing::info!(width, height, "DmaBuf FBO complete");

    Ok(DmaBuf {
        _gbm_bo: gbm_bo,
        prime_fd,
        egl_image,
        stride,
        modifier,
        rbo,
        color_tex,
        depth_rbo,
        fbo,
    })
}

impl DmaBuf {
    /// GL name of the color texture. Only valid after [`Self::allocate_texture`].
    pub fn color_tex_id(&self) -> u32 {
        self.color_tex
            .expect("DmaBuf color is a renderbuffer")
            .0
            .get()
    }

    /// Same as [`SurfaceBackend::allocate`], but color is a `TEXTURE_2D`.
    pub fn allocate_texture(renderer: &Renderer, width: u32, height: u32) -> Result<Self> {
        allocate(renderer, width, height, true)
    }
}

impl SurfaceBackend for DmaBuf {
    fn allocate(renderer: &Renderer, width: u32, height: u32) -> Result<Self> {
        allocate(renderer, width, height, false)
    }

    fn fbo(&self) -> glow::Framebuffer {
        self.fbo
    }

    fn destroy(self, renderer: &Renderer) {
        unsafe {
            renderer.gl.delete_framebuffer(self.fbo);
            renderer.gl.delete_renderbuffer(self.depth_rbo);
            if let Some(rbo) = self.rbo {
                renderer.gl.delete_renderbuffer(rbo);
            }
            if let Some(tex) = self.color_tex {
                renderer.gl.delete_texture(tex);
            }
            (renderer.fn_destroy_image)(renderer.display.as_ptr() as *const c_void, self.egl_image);
        }
    }
}
