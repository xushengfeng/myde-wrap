use tracing::{info, error};
use smithay::reexports::calloop::EventLoop;

use drm::control::{connector, crtc, Device, Mode};
use drm::buffer::Buffer;
use std::fs::{File, OpenOptions};
use std::os::unix::io::AsFd;

use crate::wayland::App;
use crate::backend::RenderBackend;
use crate::protocol::{Rect, Transform};
use crate::renderer::Renderer as MyRenderer;

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

pub struct DrmBackend {
    width: u32,
    height: u32,
    outputs: Vec<DrmOutput>,
}

impl DrmBackend {
    pub fn new() -> Self {
        Self {
            width: 1920,
            height: 1080,
            outputs: Vec::new(),
        }
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
}

impl RenderBackend for DrmBackend {
    fn name(&self) -> &str {
        "drm"
    }

    fn init(&mut self, _event_loop: &mut EventLoop<App>, _state: &mut App) -> Result<(), Box<dyn std::error::Error>> {
        self.init_drm()?;
        Ok(())
    }

    fn render_rect(&mut self, screen_index: usize, x: i32, y: i32, width: u32, height: u32, transform: &Transform) {
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

    fn dispatch(&mut self) {
        // DRM backend doesn't need dispatch
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
}
