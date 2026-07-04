use std::sync::Arc;
use std::time::Instant;
use tracing::{info, error};

use smithay::{
    backend::renderer::{
        element::{
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Kind,
        },
        gles::GlesRenderer,
        utils::{draw_render_elements, on_commit_buffer_handler},
        Color32F, Frame, Renderer,
    },
    delegate_compositor, delegate_seat, delegate_shm, delegate_xdg_shell,
    input::{Seat, SeatHandler, SeatState},
    reexports::wayland_server::Display,
    utils::{Rectangle, Serial, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            with_surface_tree_downward, CompositorClientState, CompositorHandler, CompositorState,
            SurfaceAttributes, TraversalAction,
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};
use wayland_server::{
    backend::{ClientData, ClientId, DisconnectReason},
    protocol::{
        wl_buffer,
        wl_surface::{self, WlSurface},
    },
    Client, ListeningSocket,
};
use wayland_protocols::xdg::shell::server::xdg_toplevel;

use drm::control::{connector, crtc, Device, Mode};
use drm::buffer::Buffer;
use std::fs::{File, OpenOptions};
use std::os::unix::io::AsFd;

use crate::protocol::{Rect, Transform as MyTransform, ScreenInfo};
use crate::renderer::Renderer as MyRenderer;

struct App {
    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    seat_state: SeatState<Self>,
    seat: Seat<Self>,
    start_time: Instant,
}

struct ClientState {
    compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

impl BufferHandler for App {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl XdgShellHandler for App {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        info!("新窗口创建");
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
        });
        surface.send_configure();
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}
    fn grab(&mut self, _surface: PopupSurface, _seat: wayland_server::protocol::wl_seat::WlSeat, _serial: Serial) {}
    fn reposition_request(&mut self, _surface: PopupSurface, _positioner: PositionerState, _token: u32) {}
}

impl CompositorHandler for App {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
    }
}

impl ShmHandler for App {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SeatHandler for App {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}
    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: smithay::input::pointer::CursorImageStatus) {}
}

delegate_compositor!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_xdg_shell!(App);

struct DrmDevice {
    file: File,
}

impl AsFd for DrmDevice {
    fn as_fd(&self) -> std::os::unix::io::BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl drm::Device for DrmDevice {}
impl Device for DrmDevice {}

struct DrmOutput {
    file: File,
    crtc: crtc::Handle,
    connector: connector::Handle,
    mode: Mode,
    width: u32,
    height: u32,
}

pub struct WaylandCompositor {
    width: u32,
    height: u32,
    display: Option<Display<App>>,
    state: Option<App>,
    listener: Option<ListeningSocket>,
    outputs: Vec<DrmOutput>,
}

impl WaylandCompositor {
    pub fn new() -> Self {
        info!("Wayland compositor 已创建");
        Self {
            width: 1920,
            height: 1080,
            display: None,
            state: None,
            listener: None,
            outputs: Vec::new(),
        }
    }

    pub fn init_winit(&mut self) -> anyhow::Result<()> {
        info!("初始化 Wayland compositor");

        // 创建 Wayland display
        let display: Display<App> = Display::new()?;
        let dh = display.handle();

        // 创建 compositor 状态
        let compositor_state = CompositorState::new::<App>(&dh);
        let shm_state = ShmState::new::<App>(&dh, vec![]);
        let mut seat_state = SeatState::new();
        let seat = seat_state.new_wl_seat(&dh, "myde-wrap");

        let state = App {
            compositor_state,
            xdg_shell_state: XdgShellState::new::<App>(&dh),
            shm_state,
            seat_state,
            seat,
            start_time: Instant::now(),
        };

        // 创建 socket
        let socket_name = format!("myde-wrap-{}", std::process::id());
        let listener = ListeningSocket::bind(socket_name.clone())?;
        info!("Wayland socket 创建成功: {}", socket_name);

        // 设置环境变量
        std::env::set_var("WAYLAND_DISPLAY", &socket_name);

        self.display = Some(display);
        self.state = Some(state);
        self.listener = Some(listener);

        // 初始化 DRM
        self.init_drm()?;

        Ok(())
    }

    fn init_drm(&mut self) -> anyhow::Result<()> {
        info!("初始化 DRM 显示后端");

        let drm_paths = ["/dev/dri/card0", "/dev/dri/card1", "/dev/dri/renderD128"];

        for path in &drm_paths {
            match OpenOptions::new().read(true).write(true).open(path) {
                Ok(file) => {
                    info!("成功打开 DRM 设备: {}", path);
                    let device = DrmDevice { file: file.try_clone()? };

                    let res = device.resource_handles()?;

                    // 查找所有可用的连接器
                    for &conn in res.connectors() {
                        let info = device.get_connector(conn, false)?;
                        if info.state() == connector::State::Connected && !info.modes().is_empty() {
                            let mode = info.modes()[0];
                            info!("找到连接器: {:?}, 模式: {:?}", conn, mode);

                            for &enc in info.encoders() {
                                let enc_info = device.get_encoder(enc)?;
                                let filter = enc_info.possible_crtcs();

                                for c in res.filter_crtcs(filter) {
                                    let output = DrmOutput {
                                        file: file.try_clone()?,
                                        crtc: c,
                                        connector: conn,
                                        mode,
                                        width: mode.size().0 as u32,
                                        height: mode.size().1 as u32,
                                    };
                                    self.outputs.push(output);
                                    info!("添加输出: CRTC {:?}, 连接器 {:?}", c, conn);
                                    break;
                                }
                                if !self.outputs.is_empty() {
                                    break;
                                }
                            }
                        }
                    }

                    if !self.outputs.is_empty() {
                        break;
                    }
                }
                Err(e) => {
                    info!("无法打开 {}: {}", path, e);
                }
            }
        }

        if self.outputs.is_empty() {
            return Err(anyhow::anyhow!("未找到可用的输出"));
        }

        // 使用第一个输出作为默认尺寸
        if let Some(output) = self.outputs.first() {
            self.width = output.width;
            self.height = output.height;
        }

        Ok(())
    }

    pub fn set_size(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        info!("设置渲染尺寸: {}x{}", width, height);
    }

    pub fn dispatch(&mut self) {
        // 处理新客户端连接
        if let (Some(ref listener), Some(ref display)) = (&self.listener, &self.display) {
            if let Ok(Some(stream)) = listener.accept() {
                let mut display_handle = display.handle();
                let client_state = ClientState {
                    compositor_state: CompositorClientState::default(),
                };
                if let Err(e) = display_handle.insert_client(stream, Arc::new(client_state)) {
                    error!("客户端连接失败: {}", e);
                }
            }
        }

        // 处理 Wayland 事件
        if let (Some(ref mut display), Some(ref mut state)) = (&mut self.display, &mut self.state) {
            display.dispatch_clients(state).unwrap_or_default();
            display.flush_clients().unwrap_or_default();
        }
    }

    pub fn render_rect_to_screen(&mut self, screen_index: usize, x: i32, y: i32, width: u32, height: u32, transform: &MyTransform) {
        if screen_index >= self.outputs.len() {
            error!("无效的屏幕索引: {}", screen_index);
            return;
        }

        let output = &self.outputs[screen_index];
        info!("渲染到屏幕 {}: ({}, {}) {}x{}", screen_index, x, y, width, height);

        // 计算变换后的矩形
        let (tx, ty, tw, th) = MyRenderer::compute_transformed_rect(
            &Rect { x, y, width, height },
            transform,
        );

        if let Err(e) = self.render_to_drm(output, tx, ty, tw, th) {
            error!("DRM 渲染错误: {}", e);
        }
    }

    fn render_to_drm(&self, output: &DrmOutput, x: i32, y: i32, width: u32, height: u32) -> anyhow::Result<()> {
        let device = DrmDevice { file: output.file.try_clone()? };
        let (w, h) = (output.width, output.height);
        let bpp = 32u32;

        let mut db = device.create_dumb_buffer(
            (w, h),
            drm::buffer::DrmFourcc::Argb8888,
            bpp,
        )?;

        let stride = db.pitch() as usize;
        let mut mapping = device.map_dumb_buffer(&mut db)?;

        // 清除为黑色
        for i in 0..mapping.len() {
            mapping[i] = 0;
        }

        // 绘制白色矩形
        let x = x.max(0) as u32;
        let y = y.max(0) as u32;
        let end_x = (x + width).min(w);
        let end_y = (y + height).min(h);

        for py in y..end_y {
            for px in x..end_x {
                let dst_idx = py as usize * stride + px as usize * 4;
                if dst_idx + 4 <= mapping.len() {
                    mapping[dst_idx] = 255;     // B
                    mapping[dst_idx + 1] = 255; // G
                    mapping[dst_idx + 2] = 255; // R
                    mapping[dst_idx + 3] = 255; // A
                }
            }
        }

        drop(mapping);

        let fb = device.add_framebuffer(&db, 32, 32)?;

        device.set_crtc(
            output.crtc,
            Some(fb),
            (0, 0),
            &[output.connector],
            Some(output.mode),
        )?;

        Ok(())
    }

    pub fn render_rect(&mut self, x: i32, y: i32, width: u32, height: u32) {
        // 默认渲染到第一个屏幕
        if let Some(output) = self.outputs.first() {
            if let Err(e) = self.render_to_drm(output, x, y, width, height) {
                error!("DRM 渲染错误: {}", e);
            }
        }
    }

    pub fn clear_buffer(&mut self) {}

    pub fn get_width(&self) -> u32 {
        self.width
    }

    pub fn get_height(&self) -> u32 {
        self.height
    }

    pub fn get_output_count(&self) -> usize {
        self.outputs.len()
    }
}
