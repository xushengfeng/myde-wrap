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
            element::surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
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
use crate::protocol::{Rect, ScreenInfo, Transform};
use crate::wayland::App;

/// 计算画布相关矩形（内容坐标系：原点为窗口内容（xdg window geometry）左上角，
/// 即协议 rect 的坐标系）。返回 (离屏画布矩形, 应用内容范围)：
///
/// - 应用内容范围 = 各窗口 surface tree 包围盒的并集（含阴影/悬出部分），
///   与应用大小实时同步；默认显示区域即此范围，保证画布不裁切应用；
/// - 离屏画布矩形 = 应用内容范围 ∪ SetWindowSize 声明的 (0, 0, w, h)，
///   仅作为 rect 截取的坐标空间（可突破物理屏幕）。
fn canvas_rects(state: &App, canvas_size: Option<(u32, u32)>) -> (Rect, Rect) {
    let mut x0 = 0i32;
    let mut y0 = 0i32;
    let mut x1 = 0i32;
    let mut y1 = 0i32;
    let mut has_window = false;

    for window in state.space.elements() {
        let geo = window.geometry();
        let bbox = window.bbox();
        // 内容坐标 = surface tree 坐标 - geo.loc
        let wx0 = bbox.loc.x - geo.loc.x;
        let wy0 = bbox.loc.y - geo.loc.y;
        let wx1 = wx0 + bbox.size.w;
        let wy1 = wy0 + bbox.size.h;
        if has_window {
            x0 = x0.min(wx0);
            y0 = y0.min(wy0);
            x1 = x1.max(wx1);
            y1 = y1.max(wy1);
        } else {
            (x0, y0, x1, y1) = (wx0, wy0, wx1, wy1);
            has_window = true;
        }
    }

    let app_rect = Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0) as u32,
        height: (y1 - y0).max(0) as u32,
    };

    // 离屏画布 = 应用内容范围 ∪ 声明画布 (0, 0, w, h)
    let (decl_w, decl_h) = canvas_size.unwrap_or((0, 0));
    let ox0 = x0.min(0);
    let oy0 = y0.min(0);
    let ox1 = x1.max(decl_w as i32);
    let oy1 = y1.max(decl_h as i32);
    let offscreen_rect = Rect {
        x: ox0,
        y: oy0,
        width: (ox1 - ox0).max(0) as u32,
        height: (oy1 - oy0).max(0) as u32,
    };

    (offscreen_rect, app_rect)
}

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
    Custom(crate::custom_element::CropStretchElement),
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
    fn geometry(
        &self,
        scale: smithay::utils::Scale<f64>,
    ) -> smithay::utils::Rectangle<i32, smithay::utils::Physical> {
        match self {
            MyElement::Wayland(e) => e.geometry(scale),
            MyElement::Custom(e) => e.geometry(scale),
        }
    }
    fn damage_since(
        &self,
        scale: smithay::utils::Scale<f64>,
        commit: Option<smithay::backend::renderer::utils::CommitCounter>,
    ) -> smithay::backend::renderer::utils::DamageSet<i32, smithay::utils::Physical> {
        match self {
            MyElement::Wayland(e) => e.damage_since(scale, commit),
            MyElement::Custom(e) => e.damage_since(scale, commit),
        }
    }
    fn opaque_regions(
        &self,
        scale: smithay::utils::Scale<f64>,
    ) -> smithay::backend::renderer::utils::OpaqueRegions<i32, smithay::utils::Physical> {
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

type GbmDrmCompositor = DrmCompositor<
    GbmAllocator<Arc<OwnedFd>>,
    GbmFramebufferExporter<Arc<OwnedFd>>,
    (),
    Arc<OwnedFd>,
>;

/// 每个输出的渲染运行时（与 `DrmBackend::outputs` 按下标一一对应）
struct DrmOutputRuntime {
    compositor: GbmDrmCompositor,
    needs_vblank: bool,
}

pub struct DrmBackend {
    width: u32,
    height: u32,
    outputs: Vec<DrmOutputData>,
    device: Option<DrmDevice>,
    device_fd: Option<DrmDeviceFd>,
    gbm: Option<GbmDevice<Arc<OwnedFd>>>,
    renderer: Option<GlesRenderer>,
    compositors: Vec<DrmOutputRuntime>,
    start_time: Instant,
    frame_count: u64,
    /// VBlank 事件通道，携带发生翻页的 crtc
    rx: Option<std::sync::mpsc::Receiver<crtc::Handle>>,
    tx: Option<std::sync::mpsc::Sender<crtc::Handle>>,
    pub rotate_shader: Option<smithay::backend::renderer::gles::GlesTexProgram>,
    /// 画布离屏纹理（随画布尺寸变化重建）
    pub offscreen_texture: Option<smithay::backend::renderer::gles::GlesTexture>,
    offscreen_size: Option<(u32, u32)>,
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
            compositors: Vec::new(),
            start_time: Instant::now(),
            frame_count: 0,
            rx: Some(rx),
            tx: Some(tx),
            rotate_shader: None,
            offscreen_texture: None,
            offscreen_size: None,
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
                            if let smithay::backend::drm::DrmEvent::VBlank(handle) = event {
                                let _ = tx.send(handle);
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

                                // 取第一个可用的 crtc
                                if let Some(c) = res.filter_crtcs(filter).into_iter().next() {
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

// 截取区域（内容坐标）：左上角 (x, y)、长宽 (w, h)
uniform vec4 u_region;
// 截取区域绕锚点 (x, y) 的旋转角（弧度）
uniform float u_rotation;
// 画布离屏纹理矩形（内容坐标）：原点 (x, y)、尺寸 (w, h)
uniform vec4 u_offscreen;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    // v_coords 为满屏四边形的归一化坐标 [0,1]^2：
    // 屏幕坐标 -> 截取区域坐标（拉伸填充，隐式缩放）
    vec2 region = vec2(v_coords.x * u_region.z, v_coords.y * u_region.w);

    // 区域以 (x, y) 为锚点、绕锚点旋转，采样画布内容坐标：
    // 等价于把画布绕 (x, y) 旋转 -u_rotation 后截取 (0, 0, w, h) 拉伸上屏，
    // 旋转作用在截取阶段，直角得以保持
    float c = cos(u_rotation);
    float s = sin(u_rotation);
    vec2 content = u_region.xy + vec2(region.x * c - region.y * s, region.x * s + region.y * c);

    // 内容坐标 -> 离屏纹理坐标；范围之外（应用绘制范围之外）为透明
    vec2 uv = (content - u_offscreen.xy) / u_offscreen.zw;

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
            &[
                smithay::backend::renderer::gles::UniformName::new(
                    "u_region",
                    smithay::backend::renderer::gles::UniformType::_4f,
                ),
                smithay::backend::renderer::gles::UniformName::new(
                    "u_rotation",
                    smithay::backend::renderer::gles::UniformType::_1f,
                ),
                smithay::backend::renderer::gles::UniformName::new(
                    "u_offscreen",
                    smithay::backend::renderer::gles::UniformType::_4f,
                ),
            ],
        )?;

        // 为每个输出创建独立的 DRM compositor（多屏支持）
        for output_data in &self.outputs {
            let surface = device.create_surface(
                output_data.crtc,
                output_data.mode,
                &[output_data.connector],
            )?;

            let output_mode_source = OutputModeSource::from(&output_data.smithay_output);

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

            let compositor = DrmCompositor::new(
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

            self.compositors.push(DrmOutputRuntime {
                compositor,
                needs_vblank: false,
            });
        }

        // Register outputs to space
        for output_data in &self.outputs {
            state.space.map_output(&output_data.smithay_output, (0, 0));
        }

        self.device = Some(device);
        self.device_fd = Some(device_fd);
        self.gbm = Some(gbm);
        self.renderer = Some(renderer);
        self.rotate_shader = Some(shader);

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
        // Handle DRM events sent via channel from the calloop source.
        // VBlank 按 crtc 路由到对应输出的 compositor 完成翻页。
        if let Some(rx) = self.rx.as_ref() {
            while let Ok(handle) = rx.try_recv() {
                if let Some(index) = self.outputs.iter().position(|o| o.crtc == handle) {
                    if let Some(runtime) = self.compositors.get_mut(index) {
                        runtime.needs_vblank = false;
                        let _ = runtime.compositor.frame_submitted();
                    }
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

    fn get_screens(&self) -> Vec<ScreenInfo> {
        self.outputs
            .iter()
            .map(|output| ScreenInfo {
                name: format!("DRM-{:?}", output.crtc),
                width: output.width,
                height: output.height,
                refresh_rate: output.mode.vrefresh(),
            })
            .collect()
    }

    fn get_output_count(&self) -> usize {
        self.outputs.len()
    }

    fn render_space(
        &mut self,
        state: &mut App,
        configs: &[crate::renderer::ScreenConfig],
        canvas_size: Option<(u32, u32)>,
    ) {
        self.frame_count += 1;

        // 内容坐标系原点 = 窗口内容（xdg window geometry）左上角（协议 rect 坐标系）。
        // offscreen_rect 为离屏画布矩形，app_rect 为应用内容范围（默认显示区域）
        let (offscreen_rect, app_rect) = canvas_rects(state, canvas_size);
        let canvas_valid = offscreen_rect.width > 0 && offscreen_rect.height > 0;

        let renderer = match self.renderer.as_mut() {
            Some(r) => r,
            None => return,
        };

        // Collect render elements from all windows in the space.
        // 与 smithay 的 Space 渲染保持一致：窗口原点对应 xdg window geometry 的
        // 原点，而 surface tree 的原点相对窗口原点偏移 geometry().loc
        //（CSD 阴影/边距）。渲染 surface tree 时减去该偏移，
        // 把窗口内容原点放在画布 (0, 0)。画布 1:1 渲染（scale = 1）。
        let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = state
            .space
            .elements()
            .flat_map(|window| {
                let surface = window.toplevel().unwrap().wl_surface().clone();
                let geometry_loc = window.geometry().loc;
                render_elements_from_surface_tree(
                    renderer,
                    &surface,
                    (
                        -geometry_loc.x - offscreen_rect.x,
                        -geometry_loc.y - offscreen_rect.y,
                    ),
                    smithay::utils::Scale::from(1.0),
                    1.0,
                    smithay::backend::renderer::element::Kind::Unspecified,
                )
            })
            .collect();

        // —— 第一遍：把画布内容 1:1 渲入离屏纹理（随画布尺寸变化重建）——
        let mut offscreen_texture: Option<smithay::backend::renderer::gles::GlesTexture> = None;
        let mut crop_shader: Option<smithay::backend::renderer::gles::GlesTexProgram> = None;

        if canvas_valid && self.rotate_shader.is_some() {
            let need_recreate = match (&self.offscreen_texture, self.offscreen_size) {
                (Some(_), Some(sz)) => sz != (offscreen_rect.width, offscreen_rect.height),
                _ => true,
            };
            if need_recreate {
                self.offscreen_texture = None;
                self.offscreen_size = None;
                let size = smithay::utils::Size::from((
                    offscreen_rect.width as i32,
                    offscreen_rect.height as i32,
                ));
                use smithay::backend::renderer::Offscreen;
                self.offscreen_texture = renderer
                    .create_buffer(smithay::backend::allocator::Fourcc::Argb8888, size)
                    .ok();
                if self.offscreen_texture.is_some() {
                    self.offscreen_size = Some((offscreen_rect.width, offscreen_rect.height));
                }
            }

            if let (Some(tex), Some(shader)) =
                (self.offscreen_texture.as_mut(), self.rotate_shader.as_ref())
            {
                use smithay::backend::renderer::{Bind, Frame, Renderer};
                {
                    let size = smithay::utils::Size::from((
                        offscreen_rect.width as i32,
                        offscreen_rect.height as i32,
                    ));
                    let mut target = renderer.bind(tex).unwrap();
                    let mut frame = renderer
                        .render(&mut target, size, smithay::utils::Transform::Normal)
                        .unwrap();

                    let damage = [smithay::utils::Rectangle::from_size(size)];
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

                offscreen_texture = Some(tex.clone());
                crop_shader = Some(shader.clone());
            }
        }

        // —— 第二遍：每个屏幕把（截取区域 → 屏幕）的映射渲上屏 ——
        for (index, (output_data, runtime)) in self
            .outputs
            .iter()
            .zip(self.compositors.iter_mut())
            .enumerate()
        {
            // 该输出还在等上一帧翻页则跳过
            if runtime.needs_vblank {
                continue;
            }

            let config = configs.iter().find(|c| c.screen_index == index);

            // rects[i] 与 transforms[i] 配对（缺省 rotation = 0）；
            // rects 留空（或无配置）= 整个画布（默认行为）
            let pairs: Vec<(Rect, f64)> = match config {
                Some(c) if !c.rects.is_empty() => c
                    .rects
                    .iter()
                    .enumerate()
                    .map(|(i, rect)| {
                        let rotation = c.transforms.get(i).map(|t| t.rotation).unwrap_or(0.0);
                        (rect.clone(), rotation)
                    })
                    .collect(),
                _ => vec![(app_rect.clone(), 0.0)],
            };

            let screen_size =
                smithay::utils::Size::from((output_data.width as i32, output_data.height as i32));
            let mut final_elements: Vec<MyElement> = Vec::new();

            if let (Some(tex), Some(shader)) = (offscreen_texture.as_ref(), crop_shader.as_ref()) {
                // 每个（截取区域, 旋转）对渲成一个旋转截取+拉伸填充元素。
                // render_frame 绘制时 slice 首元素在最上层，倒序入栈实现
                // “rects 靠前的在下层，靠后的在上层”。
                for (rect, rotation) in pairs.iter().rev() {
                    final_elements.push(MyElement::Custom(
                        crate::custom_element::CropStretchElement {
                            id: smithay::backend::renderer::element::Id::new(),
                            texture: tex.clone(),
                            src: smithay::utils::Rectangle::from_size(smithay::utils::Size::from(
                                (offscreen_rect.width as f64, offscreen_rect.height as f64),
                            )),
                            region: rect.clone(),
                            offscreen: offscreen_rect.clone(),
                            rotation: *rotation,
                            dst: smithay::utils::Rectangle::from_size(screen_size),
                            shader: shader.clone(),
                        },
                    ));
                }
            } else {
                // 回退：离屏纹理不可用时按拉伸逻辑直接渲 surface tree
                //（无旋转、仅首个区域）
                let (rect, _) = &pairs[0];
                let scale_x = output_data.width as f64 / (rect.width as f64).max(1.0);
                let scale_y = output_data.height as f64 / (rect.height as f64).max(1.0);
                let scale = smithay::utils::Scale::from((scale_x, scale_y));
                let loc_x = -(rect.x as f64 * scale_x).round() as i32;
                let loc_y = -(rect.y as f64 * scale_y).round() as i32;

                for window in state.space.elements() {
                    let surface = window.toplevel().unwrap().wl_surface().clone();
                    let geometry_offset: smithay::utils::Point<i32, smithay::utils::Physical> =
                        window.geometry().loc.to_physical_precise_round(scale);
                    let elems: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                        render_elements_from_surface_tree(
                            renderer,
                            &surface,
                            (loc_x - geometry_offset.x, loc_y - geometry_offset.y),
                            scale,
                            1.0,
                            smithay::backend::renderer::element::Kind::Unspecified,
                        );
                    final_elements.extend(elems.into_iter().map(MyElement::Wayland));
                }
            }

            // Render frame
            match runtime.compositor.render_frame::<_, MyElement>(
                renderer,
                &final_elements,
                [0.1, 0.1, 0.1, 1.0],
                FrameFlags::DEFAULT,
            ) {
                Ok(render_frame_result) => {
                    if !render_frame_result.is_empty {
                        // Queue the frame for display
                        if let Err(e) = runtime.compositor.queue_frame(()) {
                            error!(
                                "Frame {}: Failed to queue DRM frame for output {}: {}",
                                self.frame_count, index, e
                            );
                            continue;
                        }
                        runtime.needs_vblank = true;
                    }
                }
                Err(e) => {
                    error!(
                        "Frame {}: DRM render_frame failed for output {}: {:?}",
                        self.frame_count, index, e
                    );
                }
            }
        }

        // 无论是否出帧都给客户端发 frame 事件，避免阻塞
        if let Some(output_data) = self.outputs.first() {
            let output = &output_data.smithay_output;
            state.space.elements().for_each(|window| {
                window.send_frame(
                    output,
                    self.start_time.elapsed(),
                    Some(Duration::ZERO),
                    |_, _| Some(output.clone()),
                )
            });
        }

        state.space.refresh();
        state.popups.cleanup();
        let _ = state.display_handle.flush_clients();
    }

    fn frame_submitted(&mut self) {
        // This is called from the VBlank event handler
        // In a proper implementation, we would wait for VBlank before rendering the next frame
        // For now, we just log it
        debug!("frame_submitted called");
    }
}
