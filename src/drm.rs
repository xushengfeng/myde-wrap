use smithay::backend::allocator::{
    Format as DrmFormat, Fourcc as DrmFourcc, Modifier as DrmModifier,
};
use smithay::reexports::calloop::EventLoop;
use smithay::{
    backend::{
        allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        drm::{
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
            DrmDevice, DrmDeviceFd,
        },
        egl::{context::EGLContext, display::EGLDisplay},
        renderer::{
            element::{
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                Element,
            },
            gles::GlesRenderer,
        },
    },
    output::{Output, OutputModeSource, PhysicalProperties, Subpixel},
    reexports::drm::control::{connector, crtc},
    utils::Transform as SmithayTransform,
};
use std::fs::OpenOptions;
use std::os::unix::io::{AsFd, OwnedFd};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info};

use crate::backend::RenderBackend;
use crate::protocol::Transform;
use crate::wayland::App;

struct DrmOutputData {
    crtc: crtc::Handle,
    connector: connector::Handle,
    mode: smithay::reexports::drm::control::Mode,
    width: u32,
    height: u32,
    smithay_output: Output,
}



pub enum MyElement {
    Wayland(WaylandSurfaceRenderElement<GlesRenderer>),
    Custom(crate::custom_element::CustomRotatedElement),
}

impl smithay::backend::renderer::element::Element for MyElement {
    fn id(&self) -> &smithay::backend::renderer::element::Id {
        match self {
            MyElement::Wayland(e) => e.id(),
            MyElement::Custom(e) => e.id(),
        }
    }
    fn current_commit(&self) -> smithay::backend::renderer::utils::CommitCounter {
        match self {
            MyElement::Wayland(e) => e.current_commit(),
            MyElement::Custom(e) => e.current_commit(),
        }
    }
    fn src(&self) -> smithay::utils::Rectangle<f64, smithay::utils::Buffer> {
        match self {
            MyElement::Wayland(e) => e.src(),
            MyElement::Custom(e) => e.src(),
        }
    }
    fn transform(&self) -> smithay::utils::Transform {
        match self {
            MyElement::Wayland(e) => e.transform(),
            MyElement::Custom(e) => e.transform(),
        }
    }
    fn geometry(&self, scale: smithay::utils::Scale<f64>) -> smithay::utils::Rectangle<i32, smithay::utils::Physical> {
        match self {
            MyElement::Wayland(e) => e.geometry(scale),
            MyElement::Custom(e) => e.geometry(scale),
        }
    }
    fn damage_since(&self, scale: smithay::utils::Scale<f64>, commit: Option<smithay::backend::renderer::utils::CommitCounter>) -> smithay::backend::renderer::utils::DamageSet<i32, smithay::utils::Physical> {
        match self {
            MyElement::Wayland(e) => e.damage_since(scale, commit),
            MyElement::Custom(e) => e.damage_since(scale, commit),
        }
    }
    fn opaque_regions(&self, scale: smithay::utils::Scale<f64>) -> smithay::backend::renderer::utils::OpaqueRegions<i32, smithay::utils::Physical> {
        match self {
            MyElement::Wayland(e) => e.opaque_regions(scale).to_vec().into_iter().collect(),
            MyElement::Custom(_) => smithay::backend::renderer::utils::OpaqueRegions::default(),
        }
    }
    fn alpha(&self) -> f32 {
        match self {
            MyElement::Wayland(e) => e.alpha(),
            MyElement::Custom(e) => e.alpha(),
        }
    }
    fn kind(&self) -> smithay::backend::renderer::element::Kind {
        match self {
            MyElement::Wayland(e) => e.kind(),
            MyElement::Custom(e) => e.kind(),
        }
    }
}

impl smithay::backend::renderer::element::RenderElement<GlesRenderer> for MyElement {
    fn draw(
        &self,
        frame: &mut smithay::backend::renderer::gles::GlesFrame<'_, '_>,
        src: smithay::utils::Rectangle<f64, smithay::utils::Buffer>,
        dst: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
        damage: &[smithay::utils::Rectangle<i32, smithay::utils::Physical>],
        opaque_regions: &[smithay::utils::Rectangle<i32, smithay::utils::Physical>],
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        match self {
            MyElement::Wayland(e) => e.draw(frame, src, dst, damage, opaque_regions),
            MyElement::Custom(e) => e.draw(frame, src, dst, damage, opaque_regions),
        }
    }
}



pub struct DrmBackend {
    width: u32,
    height: u32,
    outputs: Vec<DrmOutputData>,
    device: Option<DrmDevice>,
    device_fd: Option<DrmDeviceFd>,
    gbm: Option<GbmDevice<Arc<OwnedFd>>>,
    renderer: Option<GlesRenderer>,
    drm_compositor: Option<
        DrmCompositor<
            GbmAllocator<Arc<OwnedFd>>,
            GbmFramebufferExporter<Arc<OwnedFd>>,
            (),
            Arc<OwnedFd>,
        >,
    >,
    start_time: Instant,
    frame_count: u64,
    rx: Option<std::sync::mpsc::Receiver<()>>,
    tx: Option<std::sync::mpsc::Sender<()>>,
    needs_vblank: bool,
    pub rotate_shader: Option<smithay::backend::renderer::gles::GlesTexProgram>,
    pub offscreen_texture: Option<smithay::backend::renderer::gles::GlesTexture>,
    offscreen_id: smithay::backend::renderer::element::Id,
}

// SAFETY: GlesRenderer contains raw pointers that are not Send, but it's safe to send
// across threads because the OpenGL context is managed by the EGL display and we only
// use the renderer in the render loop which runs on a single thread.
unsafe impl Send for DrmBackend {}

impl DrmBackend {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            width: 1920,
            height: 1080,
            outputs: Vec::new(),
            device: None,
            device_fd: None,
            gbm: None,
            renderer: None,
            drm_compositor: None,
            start_time: Instant::now(),
            frame_count: 0,
            rx: Some(rx),
            tx: Some(tx),
            needs_vblank: false,
            rotate_shader: None,
            offscreen_texture: None,
            offscreen_id: smithay::backend::renderer::element::Id::new(),
        }
    }
}

impl RenderBackend for DrmBackend {
    fn name(&self) -> &str {
        "drm"
    }

    fn init(
        &mut self,
        _event_loop: &mut EventLoop<App>,
        state: &mut App,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Initializing DRM display backend");

        let drm_paths = ["/dev/dri/card0", "/dev/dri/card1", "/dev/dri/renderD128"];
        let mut device: Option<DrmDevice> = None;
        let mut device_fd: Option<DrmDeviceFd> = None;

        for path in &drm_paths {
            match OpenOptions::new().read(true).write(true).open(path) {
                Ok(file) => {
                    info!("Opened DRM device: {}", path);

                    let fd: OwnedFd = file.as_fd().try_clone_to_owned()?;
                    let fd = DrmDeviceFd::new(fd.into());

                    let (dev, drm_event) = DrmDevice::new(fd.clone(), true)?;

                    let tx = self.tx.as_ref().unwrap().clone();
                    _event_loop
                        .handle()
                        .insert_source(drm_event, move |event, _meta, _state| {
                            if let smithay::backend::drm::DrmEvent::VBlank(_) = event {
                                let _ = tx.send(());
                            }
                        })
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

                    use smithay::reexports::drm::control::Device;
                    let res = dev.resource_handles()?;

                    // Find all available connectors
                    for &conn in res.connectors() {
                        let info = dev.get_connector(conn, false)?;
                        if info.state() == connector::State::Connected && !info.modes().is_empty() {
                            let mode = info.modes()[0];
                            info!("Found connector: {:?}, mode: {:?}", conn, mode);

                            for &enc in info.encoders() {
                                let enc_info = dev.get_encoder(enc)?;
                                let filter = enc_info.possible_crtcs();

                                for c in res.filter_crtcs(filter) {
                                    let smithay_mode = smithay::output::Mode {
                                        size: (mode.size().0 as i32, mode.size().1 as i32).into(),
                                        refresh: (mode.vrefresh() * 1000) as i32,
                                    };

                                    let smithay_output = Output::new(
                                        format!("DRM-{:?}", c),
                                        PhysicalProperties {
                                            size: (0, 0).into(),
                                            subpixel: Subpixel::Unknown,
                                            make: "DRM".into(),
                                            model: "Output".into(),
                                        },
                                    );

                                    smithay_output.create_global::<App>(&state.display_handle);
                                    smithay_output.change_current_state(
                                        Some(smithay_mode),
                                        Some(SmithayTransform::Normal),
                                        None,
                                        Some((0, 0).into()),
                                    );
                                    smithay_output.set_preferred(smithay_mode);

                                    let output_data = DrmOutputData {
                                        crtc: c,
                                        connector: conn,
                                        mode,
                                        width: mode.size().0 as u32,
                                        height: mode.size().1 as u32,
                                        smithay_output,
                                    };
                                    self.outputs.push(output_data);
                                    info!("Added output: CRTC {:?}, connector {:?}", c, conn);
                                    break;
                                }
                                if !self.outputs.is_empty() {
                                    break;
                                }
                            }
                        }
                    }

                    if !self.outputs.is_empty() {
                        device = Some(dev);
                        device_fd = Some(fd);
                        break;
                    }
                }
                Err(e) => {
                    debug!("Cannot open {}: {}", path, e);
                }
            }
        }

        let mut device = device.ok_or_else(|| -> Box<dyn std::error::Error> {
            "No available DRM device found".into()
        })?;
        let device_fd = device_fd.unwrap();

        if self.outputs.is_empty() {
            return Err("No available outputs found".into());
        }

        // Use first output as default size
        if let Some(output) = self.outputs.first() {
            self.width = output.width;
            self.height = output.height;
        }

        // Create GBM device from DRM fd
        let gbm_fd = device_fd.as_fd().try_clone_to_owned()?;
        let gbm: GbmDevice<Arc<OwnedFd>> = GbmDevice::new(Arc::new(gbm_fd))?;

        // Create EGL display from GBM device
        let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
        let egl_context = EGLContext::new(&egl_display)?;
        let egl_context = EGLContext::new(&egl_display)?;
        let mut renderer = unsafe { GlesRenderer::new(egl_context)? };

        let shader = renderer.compile_custom_texture_shader(
            r#"
#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;
uniform float custom_rotation;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    float c = cos(custom_rotation);
    float s = sin(custom_rotation);
    
    // Rotate around (0.0, 0.0) which corresponds to rect.xy (top-left of the screen)
    vec2 p = v_coords;
    vec2 rp = vec2(p.x * c - p.y * s, p.x * s + p.y * c);
    vec2 uv = rp;

    vec4 color;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        color = vec4(0.0);
    } else {
        color = texture2D(tex, uv);
    }

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#,
            &[smithay::backend::renderer::gles::UniformName::new(
                "custom_rotation",
                smithay::backend::renderer::gles::UniformType::_1f,
            )],
        )?;

        let offscreen_size = smithay::utils::Size::from((self.width as i32, self.height as i32));
        use smithay::backend::renderer::Offscreen;
        let offscreen_texture = renderer
            .create_buffer(
                smithay::backend::allocator::Fourcc::Argb8888,
                offscreen_size,
            )
            .ok();

        // Create DRM compositor for the first output
        let first_output = &self.outputs[0];
        let surface = device.create_surface(
            first_output.crtc,
            first_output.mode,
            &[first_output.connector],
        )?;

        let output_mode_source = OutputModeSource::from(&first_output.smithay_output);

        let allocator = GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::SCANOUT | GbmBufferFlags::RENDERING,
        );
        let framebuffer_exporter = GbmFramebufferExporter::new(gbm.clone(), None);

        // Get renderer formats
        let color_formats = [DrmFourcc::Argb8888];
        let renderer_formats = [DrmFormat {
            code: DrmFourcc::Argb8888,
            modifier: DrmModifier::Invalid,
        }];

        let drm_compositor = DrmCompositor::new(
            output_mode_source,
            surface,
            None,
            allocator,
            framebuffer_exporter,
            color_formats.into_iter(),
            renderer_formats.into_iter(),
            device.cursor_size(),
            Some(gbm.clone()),
        )?;

        // Register outputs to space
        for output_data in &self.outputs {
            state.space.map_output(&output_data.smithay_output, (0, 0));
        }

        self.device = Some(device);
        self.device_fd = Some(device_fd);
        self.gbm = Some(gbm);
        self.renderer = Some(renderer);
        self.drm_compositor = Some(drm_compositor);
        self.rotate_shader = Some(shader);
        self.offscreen_texture = offscreen_texture;

        info!(
            "DRM backend initialized, found {} outputs",
            self.outputs.len()
        );

        Ok(())
    }

    fn render_rect(
        &mut self,
        screen_index: usize,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        _transform: &Transform,
    ) {
        if screen_index >= self.outputs.len() {
            error!("Invalid screen index: {}", screen_index);
            return;
        }

        info!(
            "Render to screen {}: ({}, {}) {}x{}",
            screen_index, x, y, width, height
        );
    }

    fn dispatch(&mut self) {
        // Handle DRM events sent via channel from the calloop source
        if let Some(rx) = &self.rx {
            while let Ok(_) = rx.try_recv() {
                self.needs_vblank = false;
                if let Some(compositor) = &mut self.drm_compositor {
                    let _ = compositor.frame_submitted();
                }
            }
        }
    }

    fn get_width(&self) -> u32 {
        self.width
    }

    fn get_height(&self) -> u32 {
        self.height
    }

    fn get_output_count(&self) -> usize {
        self.outputs.len()
    }

    fn render_space(&mut self, state: &mut App, configs: &[crate::renderer::ScreenConfig]) {
        if self.needs_vblank {
            return;
        }

        let renderer = match &mut self.renderer {
            Some(r) => r,
            None => return,
        };

        let compositor = match &mut self.drm_compositor {
            Some(c) => c,
            None => return,
        };

        self.frame_count += 1;

        // Debug: Check space elements
        let element_count = state.space.elements().count();
        if self.frame_count <= 10 || element_count == 0 && self.frame_count % 60 == 0 {
            info!(
                "Frame {}: space has {} elements",
                self.frame_count, element_count
            );
            // List all windows
            for (i, window) in state.space.elements().enumerate() {
                let surface = window.toplevel().unwrap().wl_surface();
                let geo = state.space.element_geometry(window);
                info!("  Window {}: surface {:?}, geometry {:?}", i, surface, geo);
            }
            // Also check toplevel_surfaces from xdg_shell_state
            let toplevel_count = state.xdg_shell_state.toplevel_surfaces().len();
            info!("  xdg_shell_state has {} toplevel surfaces", toplevel_count);
        }

        let output_data = &self.outputs[0];
        let screen_index = 0; // For now we only render the first DRM output

        // Find config for this screen
        let config = configs.iter().find(|c| c.screen_index == screen_index);

        let mut scale_x = 1.0;
        let mut scale_y = 1.0;
        let mut loc_x = 0;
        let mut loc_y = 0;
        let mut rotation = 0.0;

        if let Some(config) = config {
            if let Some(rect) = config.rects.first() {
                let screen_w = output_data.width as f64;
                let screen_h = output_data.height as f64;

                scale_x = screen_w / (rect.width as f64).max(1.0);
                scale_y = screen_h / (rect.height as f64).max(1.0);

                loc_x = -(rect.x as f64 * scale_x).round() as i32;
                loc_y = -(rect.y as f64 * scale_y).round() as i32;
            }
            if let Some(transform) = config.transforms.first() {
                rotation = transform.rotation;
            }
        }

        // Update output transform based on rotation
        let transform = match rotation {
            r if (r >= 45.0 && r < 135.0) || (r <= -225.0 && r > -315.0) => {
                smithay::utils::Transform::_90
            }
            r if (r >= 135.0 && r < 225.0) || (r <= -135.0 && r > -225.0) => {
                smithay::utils::Transform::_180
            }
            r if (r >= 225.0 && r < 315.0) || (r <= -45.0 && r > -135.0) => {
                smithay::utils::Transform::_270
            }
            _ => smithay::utils::Transform::Normal,
        };
        output_data
            .smithay_output
            .change_current_state(None, Some(transform), None, None);

        // Collect render elements from all windows in the space
        let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = state
            .space
            .elements()
            .flat_map(|window| {
                let surface = window.toplevel().unwrap().wl_surface().clone();
                render_elements_from_surface_tree(
                    renderer,
                    &surface,
                    (loc_x, loc_y),
                    smithay::utils::Scale::from((scale_x, scale_y)),
                    1.0,
                    smithay::backend::renderer::element::Kind::Unspecified,
                )
            })
            .collect();

        // If we have an offscreen texture and a rotate shader, render to it first
        let mut final_elements: Vec<MyElement> = Vec::new();
        let mut offscreen_success = false;

        if let (Some(tex), Some(shader)) =
            (self.offscreen_texture.as_mut(), self.rotate_shader.as_ref())
        {
            use smithay::backend::renderer::{Bind, Frame, Renderer};
            {
                let mut target = renderer.bind(tex).unwrap();
                let mut frame = renderer
                    .render(
                        &mut target,
                        smithay::utils::Size::from((self.width as i32, self.height as i32)),
                        smithay::utils::Transform::Normal,
                    )
                    .unwrap();

                let damage = [smithay::utils::Rectangle::from_size(
                    smithay::utils::Size::from((self.width as i32, self.height as i32)),
                )];
                let _ = frame.clear(
                    smithay::backend::renderer::Color32F::new(0.0, 0.0, 0.0, 0.0),
                    &damage,
                );

                for element in &elements {
                    use smithay::backend::renderer::element::{Element, RenderElement};
                    let _ = element.draw(
                        &mut frame,
                        element.src(),
                        element.geometry(smithay::utils::Scale::from(1.0)),
                        &damage,
                        &[],
                    );
                }
            }

            offscreen_success = true;
            final_elements.push(MyElement::Custom(crate::custom_element::CustomRotatedElement {
                id: self.offscreen_id.clone(),
                texture: tex.clone(),
                src: smithay::utils::Rectangle::from_size(smithay::utils::Size::from((
                    self.width as f64,
                    self.height as f64,
                ))),
                dst: smithay::utils::Rectangle::from_size(smithay::utils::Size::from((
                    self.width as i32,
                    self.height as i32,
                ))),
                rotation,
                shader: shader.clone(),
            }));
        }

        if !offscreen_success {
            for element in elements {
                final_elements.push(MyElement::Wayland(element));
            }
        }

        // Render frame
        match compositor.render_frame::<_, MyElement>(
            renderer,
            &final_elements,
            [0.1, 0.1, 0.1, 1.0],
            FrameFlags::DEFAULT,
        ) {
            Ok(render_frame_result) => {
                if self.frame_count <= 10 {
                    info!(
                        "Frame {}: render_frame returned is_empty={}",
                        self.frame_count, render_frame_result.is_empty
                    );
                }
                if !render_frame_result.is_empty {
                    if self.frame_count <= 10 || self.frame_count % 60 == 0 {
                        info!("Frame {}: queuing frame", self.frame_count);
                    }
                    // Queue the frame for display
                    if let Err(e) = compositor.queue_frame(()) {
                        error!(
                            "Frame {}: Failed to queue DRM frame: {}",
                            self.frame_count, e
                        );
                        return;
                    }
                    self.needs_vblank = true;

                    if self.frame_count <= 10 {
                        info!("Frame {}: Frame queued successfully", self.frame_count);
                    }
                }

                // Always send frame events to Wayland clients so they are not blocked
                let output = &self.outputs[0].smithay_output;
                state.space.elements().for_each(|window| {
                    window.send_frame(
                        output,
                        self.start_time.elapsed(),
                        Some(Duration::ZERO),
                        |_, _| Some(output.clone()),
                    )
                });

                state.space.refresh();
                state.popups.cleanup();
                let _ = state.display_handle.flush_clients();
            }
            Err(e) => {
                error!(
                    "Frame {}: DRM render_frame failed: {:?}",
                    self.frame_count, e
                );
            }
        }
    }

    fn frame_submitted(&mut self) {
        // This is called from the VBlank event handler
        // In a proper implementation, we would wait for VBlank before rendering the next frame
        // For now, we just log it
        debug!("frame_submitted called");
    }
}
