use std::sync::Arc;
use tokio::sync::Mutex;

use crate::protocol::{ClientMessage, ServerResponse};
use crate::renderer::Renderer;

pub struct Compositor {
    renderer: Arc<Mutex<Renderer>>,
}

impl Compositor {
    pub fn new(renderer: Arc<Mutex<Renderer>>) -> Self {
        Self { renderer }
    }

    pub async fn handle_message(&self, msg: ClientMessage) -> ServerResponse {
        let mut renderer = self.renderer.lock().await;

        match msg {
            ClientMessage::SetWindowSize { width, height } => {
                renderer.set_window_size(width, height);
                ServerResponse::WindowSizeSet { width, height }
            }
            ClientMessage::CaptureRects { rects } => {
                let captured = renderer.capture_rects(rects);
                ServerResponse::RectsCaptured { rects: captured }
            }
            ClientMessage::TransformRects { transforms } => {
                renderer.transform_rects(transforms);
                ServerResponse::RectsTransformed
            }
            ClientMessage::RenderToScreen { screen_index, rects, transforms } => {
                match renderer.render_to_screen(screen_index, rects, transforms) {
                    Ok(()) => ServerResponse::RenderedToScreen { screen_index },
                    Err(e) => ServerResponse::Error { message: e.to_string() },
                }
            }
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
