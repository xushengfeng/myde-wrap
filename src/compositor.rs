use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::protocol::{ClientMessage, ServerResponse};
use crate::renderer::Renderer;

pub struct Compositor {
    renderer: Arc<Mutex<Renderer>>,
    /// 配置变化后置位，通知渲染线程（calloop）重新上屏
    redraw: Arc<AtomicBool>,
}

impl Compositor {
    pub fn new(renderer: Arc<Mutex<Renderer>>, redraw: Arc<AtomicBool>) -> Self {
        Self { renderer, redraw }
    }

    fn invalidate(&self) {
        self.redraw.store(true, Ordering::Release);
    }

    pub async fn handle_message(&self, msg: ClientMessage) -> ServerResponse {
        let mut renderer = self.renderer.lock().await;

        match msg {
            ClientMessage::SetWindowSize { width, height } => {
                renderer.set_window_size(width, height);
                self.invalidate();
                ServerResponse::WindowSizeSet { width, height }
            }
            ClientMessage::CaptureRects { rects } => {
                let captured = renderer.capture_rects(rects);
                self.invalidate();
                ServerResponse::RectsCaptured { rects: captured }
            }
            ClientMessage::TransformRects { transforms } => {
                renderer.transform_rects(transforms);
                self.invalidate();
                ServerResponse::RectsTransformed
            }
            ClientMessage::RenderToScreen {
                screen_index,
                rect,
                transform,
            } => match renderer.render_to_screen(screen_index, rect, transform) {
                Ok(()) => {
                    self.invalidate();
                    ServerResponse::RenderedToScreen { screen_index }
                }
                Err(e) => ServerResponse::Error {
                    message: e.to_string(),
                },
            },
            ClientMessage::GetScreens => {
                let screens = renderer.get_screens();
                ServerResponse::Screens { screens }
            }
            ClientMessage::SetInputEnabled { enabled } => {
                renderer.set_input_enabled(enabled);
                ServerResponse::InputState { enabled }
            }
            ClientMessage::Ping => ServerResponse::Pong,
        }
    }
}
