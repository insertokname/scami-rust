use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop};
use winit::window::{Window, WindowId};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;
const TARGET_FPS: u64 = 240;
const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000u64 / TARGET_FPS);

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

struct App {
    window: Option<Rc<Window>>,
    surface: Option<Surface>,
    start_time: Instant,
    last_frame_time: Instant,
    frame_count: u32,
    fps_timer: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Rc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("SCAM")
                        .with_inner_size(winit::dpi::LogicalSize::new(WIDTH as f64, HEIGHT as f64)),
                )
                .unwrap(),
        );

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        self.window = Some(window);
        self.surface = Some(surface);

        let now = Instant::now();
        self.last_frame_time = now;
        self.fps_timer = now;

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let window = match &self.window {
            Some(window) if window.id() == window_id => window,
            _ => return,
        };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(surface) = &mut self.surface {
                    if let (Some(width), Some(height)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    {
                        surface.resize(width, height).unwrap();
                    }
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let frame_start = Instant::now();
                let delta = frame_start.saturating_duration_since(self.last_frame_time);

                if let Some(surface) = &mut self.surface {
                    let size = window.inner_size();
                    if let (Some(_), Some(_)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    {
                        let time = self.start_time.elapsed().as_secs_f32();
                        let red = ((time.sin() * 0.5 + 0.5) * 255.0) as u32;
                        let green = (((time + 2.094).sin() * 0.5 + 0.5) * 255.0) as u32;
                        let blue = (((time + 4.188).sin() * 0.5 + 0.5) * 255.0) as u32;
                        let color = (red << 16) | (green << 8) | blue;

                        if let Ok(mut buffer) = surface.buffer_mut() {
                            buffer.fill(color);
                            let _ = buffer.present();
                        }
                    }
                }

                let scheduled_next_start = self.last_frame_time + FRAME_TIME;
                if frame_start < scheduled_next_start {
                    let remaining = scheduled_next_start - frame_start;

                    if remaining > Duration::from_micros(500) {
                        let coarse = remaining - Duration::from_micros(200);
                        std::thread::sleep(coarse);
                    }
                    while Instant::now() < scheduled_next_start {
                        std::hint::spin_loop();
                    }
                    self.last_frame_time = scheduled_next_start;
                } else {
                    self.last_frame_time = frame_start;
                }

                self.frame_count += 1;
                let now_stats = Instant::now();
                if now_stats.duration_since(self.fps_timer) >= Duration::from_secs(1) {
                    let avg_ms = (now_stats - self.fps_timer).as_secs_f64() * 1000.0
                        / self.frame_count as f64;
                    println!(
                        "FPS: {} | avg frame {:.3} ms | last delta {:.3} ms",
                        self.frame_count,
                        avg_ms,
                        delta.as_secs_f64() * 1000.0
                    );
                    self.frame_count = 0;
                    self.fps_timer = now_stats;
                }

                window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() {
    // let event_loop = EventLoop::new().unwrap();

    // let now = Instant::now();
    // let mut app = App {
    //     window: None,
    //     surface: None,
    //     start_time: now,
    //     last_frame_time: now,
    //     frame_count: 0,
    //     fps_timer: now,
    // };

    // event_loop.run_app(&mut app).unwrap();

    // println!("{}", 1 << 6 == 0x60)
    let mut nes = scamu::device::nes::Nes::new();
    nes.write_memory(
        0x8000,
        &[
            0xA9, 0x00, 0x85, 0x00, 0xA9, 0x01, 0x85, 0x01, 0xA2, 0x00, 0xB5, 0x00, 0x18, 0x75,
            0x01, 0x95, 0x02, 0xE8, 0x90, 0xF6, 0xE8,
        ],
    );

    nes.write_memory(0xFFFC, &[0x00, 0x80]);

    nes.reset();


    loop {
        nes.tick();
    }
}
