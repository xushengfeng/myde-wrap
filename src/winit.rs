use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement, gles::GlesRenderer,
        },
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform as SmithayTransform},
};

use crate::wayland::App;
use crate::backend::RenderBackend;
use crate::protocol::Transform;

pub struct WinitBackend {
    width: u32,
    height: u32,
    output: Option<Output>,
}

impl WinitBackend {
    pub fn new() -> Self {
        Self {
            width: 800,
            height: 600,
            output: None,
        }
    }
}

impl RenderBackend for WinitBackend {
    fn name(&self) -> &str {
        "winit"
    }

    fn init(&mut self, event_loop: &mut EventLoop<App>, state: &mut App) -> Result<(), Box<dyn std::error::Error>> {
        let (mut backend, winit) = winit::init()?;

        let mode = Mode {
            size: backend.window_size(),
            refresh: 60_000,
        };

        self.width = mode.size.w as u32;
        self.height = mode.size.h as u32;

        let output = Output::new(
            "winit".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Smithay".into(),
                model: "Winit".into(),
            },
        );
        let _global = output.create_global::<App>(&state.display_handle);

        output.change_current_state(Some(mode), Some(SmithayTransform::Flipped180), None, Some((0, 0).into()));
        output.set_preferred(mode);

        state.space.map_output(&output, (0, 0));

        self.output = Some(output.clone());

        let mut damage_tracker = OutputDamageTracker::from_output(&output);

        event_loop.handle().insert_source(winit, move |event, _, state| {
            match event {
                WinitEvent::Resized { size, .. } => {
                    output.change_current_state(
                        Some(Mode {
                            size,
                            refresh: 60_000,
                        }),
                        None,
                        None,
                        None,
                    );
                }
                WinitEvent::Input(event) => state.process_input_event(event),
                WinitEvent::Redraw => {
                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);

                    {
                        let (renderer, mut framebuffer) = backend.bind().unwrap();
                        smithay::desktop::space::render_output::<
                            _,
                            WaylandSurfaceRenderElement<GlesRenderer>,
                            _,
                            _,
                        >(
                            &output,
                            renderer,
                            &mut framebuffer,
                            1.0,
                            0,
                            [&state.space],
                            &[],
                            &mut damage_tracker,
                            [0.1, 0.1, 0.1, 1.0],
                        )
                        .unwrap();
                    }

                    backend.submit(Some(&[damage])).unwrap();

                    state.space.elements().for_each(|window| {
                        window.send_frame(
                            &output,
                            state.start_time.elapsed(),
                            Some(Duration::ZERO),
                            |_, _| Some(output.clone()),
                        )
                    });

                    state.space.refresh();
                    state.popups.cleanup();

                    let _ = state.display_handle.flush_clients();

                    // Ask for redraw to schedule new frame.
                    backend.window().request_redraw();
                }
                WinitEvent::CloseRequested => {
                    state.loop_signal.stop();
                }
                _ => (),
            };
        })?;

        Ok(())
    }

    fn render_rect(&mut self, screen_index: usize, x: i32, y: i32, width: u32, height: u32, transform: &Transform) {
        // Winit backend handles rendering through the event loop
        // This is a no-op for winit backend
        tracing::debug!("WinitBackend::render_rect({}, {}, {}, {}, {}, {:?})", screen_index, x, y, width, height, transform);
    }

    fn dispatch(&mut self) {
        // Winit backend handles dispatch through the event loop
    }

    fn get_width(&self) -> u32 {
        self.width
    }

    fn get_height(&self) -> u32 {
        self.height
    }

    fn get_output_count(&self) -> usize {
        if self.output.is_some() { 1 } else { 0 }
    }
}
