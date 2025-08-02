use evdev::{Device, InputEventKind, Key, RelativeAxisType};
use std::error::Error;
use std::io;
use std::sync::mpsc::{channel};
use std::thread;
use std::time::{Duration, Instant};
use uinput::event::relative::Wheel;

const DEADZONE: f32 = 50.0;
const BASE_SCROLL_SPEED: f32 = 0.05;
const MAX_SCROLL_SPEED: i32 = 5;

fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting autoscroll program...");

    let mouse_path = find_mouse_device()?;
    println!("Opening mouse device: {}", mouse_path);
    let mut input = Device::open(&mouse_path)?;

    println!("Monitoring mouse events (mouse will work normally)");

    let mut uinput_dev = create_uinput_device()?;
    println!("Ready! Press middle mouse button to scroll.");

    let (tx, rx) = channel::<ScrollCommand>();

    thread::spawn(move || {
        scroll_thread(&mut uinput_dev, rx);
    });

    let mut scrolling = false;
    let mut origin_y = 0.0_f32;
    let mut absolute_y = 0.0_f32;

    loop {
        for ev in input.fetch_events()?.collect::<Vec<_>>() {
            match ev.kind() {
                InputEventKind::Key(Key::BTN_MIDDLE) => {
                    scrolling = ev.value() == 1;
                    if scrolling {
                        origin_y = absolute_y;   // mark starting Y
                        println!("Start scroll at {}", origin_y);
                        tx.send(ScrollCommand::Start)?;
                    } else {
                        println!("Stop scroll");
                        tx.send(ScrollCommand::Stop)?;
                    }
                }
                InputEventKind::RelAxis(RelativeAxisType::REL_Y) => {
                    absolute_y += ev.value() as f32;

                    if scrolling {
                        let distance = absolute_y - origin_y;

                        if distance.abs() > DEADZONE {
                            let speed =
                                ((distance.abs() - DEADZONE) * BASE_SCROLL_SPEED)
                                    .min(MAX_SCROLL_SPEED as f32) as i32;
                            let speed = speed.max(1);


                            let direction = if distance < 0.0 { 1 } else { -1 };
                            tx.send(ScrollCommand::Update(direction * speed))?;
                        } else {
                            tx.send(ScrollCommand::Update(0))?;
                        }
                    }
                }
                _ => {}
            }
        }

        thread::sleep(Duration::from_millis(5));
    }
}


#[derive(Clone, Copy)]
enum ScrollCommand {
    Start,
    Stop,
    Update(i32),
}

fn scroll_thread(uinput_dev: &mut uinput::Device, rx: std::sync::mpsc::Receiver<ScrollCommand>) {
    const SCROLL_INTERVAL: Duration = Duration::from_millis(50);
    let mut last_scroll = Instant::now();
    let mut scrolling = false;
    let mut scroll_value = 0;

    loop {
        // Check for new commands
        while let Ok(command) = rx.try_recv() {
            match command {
                ScrollCommand::Start => {
                    scrolling = true;
                    last_scroll = Instant::now();
                }
                ScrollCommand::Stop => {
                    scrolling = false;
                    scroll_value = 0;
                }
                ScrollCommand::Update(new_value) => {
                    scroll_value = new_value;
                }
            }
        }

        // Perform scrolling if active
        if scrolling && last_scroll.elapsed() >= SCROLL_INTERVAL && scroll_value != 0 {
            if let Err(e) = uinput_dev.send(Wheel::Vertical, scroll_value) {
                eprintln!("Failed to send scroll event: {}", e);
                break;
            }
            if let Err(e) = uinput_dev.synchronize() {
                eprintln!("Failed to synchronize uinput device: {}", e);
                break;
            }
            last_scroll = Instant::now();
        }

        thread::sleep(Duration::from_millis(5));
    }
}

fn find_mouse_device() -> io::Result<String> {
    use std::fs;

    let mut mouse_candidates = Vec::new();

    for entry in fs::read_dir("/dev/input")? {
        let entry = entry?;
        let path = entry.path();

        if let Some(filename) = path.file_name() {
            if let Some(filename_str) = filename.to_str() {
                if filename_str.starts_with("event") {
                    if let Ok(device) = Device::open(&path) {
                        let has_mouse_buttons = device.supported_keys().map_or(false, |keys| {
                            keys.contains(Key::BTN_LEFT)
                                || keys.contains(Key::BTN_MIDDLE)
                                || keys.contains(Key::BTN_RIGHT)
                        });

                        let has_relative_movement =
                            device.supported_relative_axes().map_or(false, |axes| {
                                axes.contains(RelativeAxisType::REL_X)
                                    && axes.contains(RelativeAxisType::REL_Y)
                            });

                        if has_mouse_buttons && has_relative_movement {
                            let device_name = device.name().unwrap_or("Unknown");
                            println!(
                                "Found potential mouse device: {} ({})",
                                path.display(),
                                device_name
                            );

                            let priority = if device_name.to_lowercase().contains("keyboard") {
                                1
                            } else {
                                2
                            };

                            mouse_candidates.push((
                                priority,
                                path.to_string_lossy().to_string(),
                                device_name.to_string(),
                            ));
                        }
                    }
                }
            }
        }
    }

    mouse_candidates.sort_by(|a, b| b.0.cmp(&a.0));

    if let Some((_, path, name)) = mouse_candidates.first() {
        println!("Selected mouse device: {} ({})", path, name);
        Ok(path.clone())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No mouse device found",
        ))
    }
}

fn create_uinput_device() -> Result<uinput::Device, uinput::Error> {
    println!("Creating uinput device...");

    if !std::path::Path::new("/dev/uinput").exists() {
        eprintln!(
            "Warning: /dev/uinput does not exist. You may need to load the uinput kernel module:"
        );
        eprintln!("  sudo modprobe uinput");
    }

    let device = uinput::default()?
        .name("autoscroll-device")?
        .event(uinput::event::relative::Wheel::Vertical)?
        .create()?;

    println!("Successfully created uinput device");
    Ok(device)
}