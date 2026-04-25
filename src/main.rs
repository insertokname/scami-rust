#![allow(dead_code)]

use rodio::source::{SawtoothWave, SquareWave, TriangleWave};
use scamu::devices::nes::Nes;
use scamu::hardware::cartrige::Cartrige;
use scamu::hardware::constants::controller::buttons;
use scamu::hardware::constants::ppu::COLORS;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::test_logger::TestLogger;

pub mod test_logger;

const PALLET_WIDTH: usize = 16 * 8;
const PALLET_HEIGHT: usize = 16 * 8;
const NAMETABLE_WIDTH: usize = 32 * 8;
const NAMETABLE_HEIGHT: usize = 30 * 8;
const INIT_WIDTH: usize = PALLET_WIDTH + NAMETABLE_WIDTH;
const INIT_HEIGHT: usize = PALLET_HEIGHT * 2;
const NES_PPU_TICKS_PER_FRAME: usize = 341 * 262;
const TARGET_FPS: u64 = 60;
const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TARGET_FPS);

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

struct App {
    window: Option<Rc<Window>>,
    surface: Option<Surface>,
    next_frame_time: Instant,
    nes: Nes,
    logical_buffer: [u32; INIT_WIDTH * INIT_HEIGHT],
    surface_width: usize,
    surface_height: usize,
}

fn scale_nearest(
    src: &[u32],
    src_w: usize,
    src_h: usize,
    dst: &mut [u32],
    dst_w: usize,
    dst_h: usize,
) {
    for y in 0..dst_h {
        let src_y = y * src_h / dst_h;
        for x in 0..dst_w {
            let src_x = x * src_w / dst_w;
            dst[y * dst_w + x] = src[src_y * src_w + src_x];
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Rc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("SCAM")
                        .with_min_inner_size(PhysicalSize::new(
                            INIT_WIDTH as u32,
                            INIT_HEIGHT as u32,
                        ))
                        .with_inner_size(winit::dpi::LogicalSize::new(
                            INIT_WIDTH as f64,
                            INIT_HEIGHT as f64,
                        )),
                )
                .unwrap(),
        );

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        self.window = Some(window);
        self.surface = Some(surface);

        self.next_frame_time = Instant::now() + FRAME_DURATION;
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame_time));

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            let now = Instant::now();
            if now >= self.next_frame_time {
                window.request_redraw();
                while self.next_frame_time <= now {
                    self.next_frame_time += FRAME_DURATION;
                }
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame_time));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let window = match &self.window {
            Some(window) if window.id() == window_id => window.clone(),
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
                        self.surface_height = size.height as usize;
                    }
                    self.surface_width = size.width as usize;
                }
                window.request_redraw();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: code,
                        state,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;

                if let PhysicalKey::Code(keycode) = code {
                    if self.handle_controller_key(keycode, pressed) {
                        return;
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_frame(&window);
            }
            _ => {}
        }
    }
}

impl App {
    fn handle_controller_key(&mut self, key: KeyCode, pressed: bool) -> bool {
        let button = match key {
            KeyCode::KeyW => Some(buttons::UP),
            KeyCode::ArrowUp => Some(buttons::UP),
            KeyCode::KeyA => Some(buttons::LEFT),
            KeyCode::ArrowLeft => Some(buttons::LEFT),
            KeyCode::KeyS => Some(buttons::DOWN),
            KeyCode::ArrowDown => Some(buttons::DOWN),
            KeyCode::KeyD => Some(buttons::RIGHT),
            KeyCode::ArrowRight => Some(buttons::RIGHT),
            KeyCode::KeyZ => Some(buttons::A),
            KeyCode::KeyJ => Some(buttons::A),
            KeyCode::KeyX => Some(buttons::B),
            KeyCode::KeyK => Some(buttons::B),
            KeyCode::KeyC => Some(buttons::START),
            KeyCode::Enter => Some(buttons::START),
            KeyCode::KeyV => Some(buttons::SELECT),
            KeyCode::ShiftRight => Some(buttons::SELECT),
            _ => None,
        };

        if let Some(button) = button {
            self.nes.bus.set_controller_button(0, button, pressed);
            return true;
        }

        false
    }

    fn render_frame(&mut self, window: &Rc<Window>) {
        if let Some(surface) = &mut self.surface {
            let size = window.inner_size();
            if let (Some(_), Some(_)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
            {
                let mut buffer = match surface.buffer_mut() {
                    Ok(buffer) => buffer,
                    Err(..) => return,
                };

                for _ in 0..NES_PPU_TICKS_PER_FRAME {
                    let out = self.nes.tick();
                    if let Some(pix) = out {
                        let color_index = self
                            .nes
                            .ppu
                            .borrow()
                            .pallet_memory
                            .read_index(pix.3 as u16, pix.2 as u16)
                            as usize;
                        let color = COLORS[color_index];
                        self.logical_buffer[pix.1 as usize * INIT_WIDTH + pix.0 as usize] = color;
                    }
                }

                scale_nearest(
                    &self.logical_buffer,
                    INIT_WIDTH,
                    INIT_HEIGHT,
                    &mut buffer,
                    self.surface_width,
                    self.surface_height,
                );
                let _ = buffer.present();
            }
        }
    }
}

static NESTEST_TEST_LOGGER: TestLogger = TestLogger::new();

fn main() {
    // use rodio::source::{SineWave, Source};
    // use rodio::{MixerDeviceSink, Player};
    // use std::time::Duration;

    // // _stream must live as long as the sink
    // let handle = rodio::DeviceSinkBuilder::open_default_sink().expect("open default audio stream");
    // let player = rodio::Player::connect_new(&handle.mixer());

    // // Add a dummy source of the sake of the example.
    // let source = SawtoothWave::new(500.0).amplify(0.2);
    // player.append(source);

    // player

    // // source.amplify(0.5);

    // // The sound plays in a separate thread. This call will block the current thread until the
    // // player has finished playing all its queued sounds.
    // // player.sleep_until_end();

    // thread::sleep(Duration::from_secs(5));

    // player.stop();

    // log::set_logger(&NESTEST_TEST_LOGGER).unwrap();
    // log::set_max_level(log::LevelFilter::Trace);

    let event_loop = EventLoop::new().unwrap();

    let now = Instant::now();
    let mut app = App {
        window: None,
        surface: None,
        next_frame_time: now + FRAME_DURATION,
        nes: Nes::new(),
        logical_buffer: [0; INIT_WIDTH * INIT_HEIGHT],
        surface_width: INIT_WIDTH,
        surface_height: INIT_HEIGHT,
    };

    // let cartrige = Cartrige::from_bytes(include_bytes!("./nestest.nes")).unwrap();
    // let cartrige = Cartrige::from_bytes(include_bytes!("./AccuracyCoin.nes")).unwrap();
    let cartrige = Cartrige::from_bytes(include_bytes!("./gitignored_games/smb.nes")).unwrap();
    // let cartrige = Cartrige::from_bytes(include_bytes!("./gitignored_games/pacman.nes")).unwrap();
    // let cartrige = Cartrige::from_bytes(include_bytes!("./gitignored_games/dk.nes")).unwrap();
    // let cartrige = Cartrige::from_bytes(include_bytes!("./gitignored_games/ic.nes")).unwrap();
    // let cartrige = Cartrige::from_bytes(include_bytes!("./gitignored_games/tetris-73.nes")).unwrap();
    app.nes.insert_cartrige(cartrige);
    app.nes.reset();

    event_loop.run_app(&mut app).unwrap();
}
