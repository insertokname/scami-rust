#![allow(dead_code)]

use scamu::devices::nes::Nes;
use scamu::hardware::cartrige::Cartrige;
use scamu::hardware::constants::controller::buttons;
use scamu::hardware::constants::ppu::COLORS;
use pixels::{Pixels, SurfaceTexture};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, StartCause, WindowEvent};
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
const NES_MASTER_CLOCK_HZ: u64 = 21_477_272;
const MASTER_CLOCKS_PER_NES_TICK: u64 = 4;
const NES_TICK_HZ: u64 = NES_MASTER_CLOCK_HZ / MASTER_CLOCKS_PER_NES_TICK;
const NANOS_PER_SECOND: u128 = 1_000_000_000;
const LAST_VISIBLE_X: u32 = 255;
const LAST_VISIBLE_Y: u32 = 239;
const RUN_UNCAPPED: bool = false;

struct App {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    emulation_anchor: Instant,
    completed_ticks: u64,
    next_tick_deadline: Instant,
    nes: Nes,
    draw_buffer: [u8; INIT_WIDTH * INIT_HEIGHT * 4],
    latched_buffer: [u8; INIT_WIDTH * INIT_HEIGHT * 4],
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        match cause {
            StartCause::Init
            | StartCause::ResumeTimeReached { .. }
            | StartCause::WaitCancelled { .. }
            | StartCause::Poll => {
                self.run_due_ticks();
                self.configure_control_flow(event_loop);
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
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

        let initial_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(
            initial_size.width.max(1),
            initial_size.height.max(1),
            window.clone(),
        );
        let mut pixels = Pixels::new(
            INIT_WIDTH as u32,
            INIT_HEIGHT as u32,
            surface_texture,
        )
        .unwrap();
        if RUN_UNCAPPED {
            pixels.enable_vsync(false);
        } else {
            pixels.enable_vsync(true);
            pixels.set_present_mode(pixels::wgpu::PresentMode::Fifo);
        }

        self.window = Some(window);
        self.pixels = Some(pixels);

        self.emulation_anchor = Instant::now();
        self.completed_ticks = 0;
        self.next_tick_deadline = self.deadline_for_tick(1);

        self.configure_control_flow(event_loop);

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.configure_control_flow(event_loop);
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
                if size.width > 0
                    && size.height > 0
                    && let Some(pixels) = &mut self.pixels
                {
                    let _ = pixels.resize_surface(size.width, size.height);
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
                self.present_buffer();
            }
            _ => {}
        }
    }
}

impl App {
    fn configure_control_flow(&self, event_loop: &ActiveEventLoop) {
        if RUN_UNCAPPED {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick_deadline));
        }
    }

    fn deadline_for_tick(&self, tick_number: u64) -> Instant {
        let nanos = (tick_number as u128 * NANOS_PER_SECOND) / NES_TICK_HZ as u128;
        self.emulation_anchor + Duration::from_nanos(nanos.min(u64::MAX as u128) as u64)
    }

    fn tick_once(&mut self) -> bool {
        let out = self.nes.tick();
        self.completed_ticks = self.completed_ticks.saturating_add(1);

        if let Some((x, y, pattern, attrib)) = out {
            let color_index = self
                .nes
                .ppu
                .borrow()
                .pallet_memory
                .read_index(attrib as u16, pattern as u16) as usize;

            let color = COLORS[color_index];
            let i = (y as usize * INIT_WIDTH + x as usize) * 4;
            self.draw_buffer[i] = ((color >> 16) & 0xFF) as u8;
            self.draw_buffer[i + 1] = ((color >> 8) & 0xFF) as u8;
            self.draw_buffer[i + 2] = (color & 0xFF) as u8;
            self.draw_buffer[i + 3] = 0xFF;

            if x == LAST_VISIBLE_X && y == LAST_VISIBLE_Y {
                std::mem::swap(&mut self.draw_buffer, &mut self.latched_buffer);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return true;
            }
        }

        false
    }

    fn run_due_ticks(&mut self) {
        if self.window.is_none() {
            return;
        }

        if RUN_UNCAPPED {
            while !self.tick_once() {}
            return;
        }

        let elapsed_nanos = Instant::now()
            .saturating_duration_since(self.emulation_anchor)
            .as_nanos();
        let target_ticks = (elapsed_nanos * NES_TICK_HZ as u128 / NANOS_PER_SECOND) as u64;

        while self.completed_ticks < target_ticks {
            self.tick_once();
        }

        self.next_tick_deadline = self.deadline_for_tick(self.completed_ticks + 1);
    }

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

    fn present_buffer(&mut self) {
        if let Some(pixels) = &mut self.pixels {
            let frame = pixels.frame_mut();
            frame.copy_from_slice(&self.latched_buffer);
            let _ = pixels.render();
        }
    }
}

static NESTEST_TEST_LOGGER: TestLogger = TestLogger::new();

fn main() {
    // use rodio::source::{SineWave, Source};
    // use std::time::Duration;

    // // _stream must live as long as the sink
    // let handle = rodio::DeviceSinkBuilder::open_default_sink().expect("open default audio stream");
    // let player = rodio::Player::connect_new(&handle.mixer());

    // // Add a dummy source of the sake of the example.
    // let source = SineWave::new(200.0).amplify(0.1);
    // player.append(source);

    // // player

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
        pixels: None,
        emulation_anchor: now,
        completed_ticks: 0,
        next_tick_deadline: now,
        nes: Nes::new(),
        draw_buffer: [0; INIT_WIDTH * INIT_HEIGHT * 4],
        latched_buffer: [0; INIT_WIDTH * INIT_HEIGHT * 4],
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
