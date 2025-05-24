use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::process::exit;
use std::thread::{self, sleep};
use std::time::Duration;

mod ui;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const SAMPLE_RATE: u32 = 48_000;
// Reactive
const CHUNK_SIZE: Duration = Duration::from_millis(50);

fn main() {
    thread::spawn(|| {
        ui::run_ui();
    });

    let host = cpal::default_host();

    match host.input_devices() {
        Ok(devices) => {
            for (i, device) in devices.enumerate() {
                match device.name() {
                    Ok(name) => println!("{i}. {name}"),
                    Err(e) => eprintln!("{i}. Error getting name: {e}"),
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to get input devices: {e}");
        }
    }

    let device = host
        .input_devices()
        .expect("Failed to get input devices")
        .find(|d| d.name().map(|name| name.contains("NTUSB")).unwrap_or(false))
        .expect("No input devices available");

    println!("Using input device: {}", device.name().unwrap());

    let mut supported_config_range = device
        .supported_input_configs()
        .expect("Error while querying configs");

    let supported_config = supported_config_range
        .next()
        .expect("No supported config available")
        .with_sample_rate(cpal::SampleRate(SAMPLE_RATE));

    println!("sampleformat: {}", supported_config.sample_format());
    println!("samplerate:   {}", supported_config.sample_format());

    let mut buffer = Vec::new();
    // Sample rate * duration in seconds = number of samples in duration.
    let samples_per_chunk: usize = (SAMPLE_RATE as usize * CHUNK_SIZE.as_millis() as usize) / 1000;

    let mut state = RmsState::default();
    state.min_rms = 0.0;
    state.max_rms = 0.9;

    let stream = device
        .build_input_stream(
            &supported_config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                buffer.extend_from_slice(data);

                while buffer.len() >= samples_per_chunk {
                    let chunk: Vec<f32> = buffer.drain(..samples_per_chunk).collect();
                    process_audio_chunk(&chunk, &mut state);
                }
            },
            move |err| {
                eprintln!("Stream error: {err}");
            },
            None,
        )
        .expect("Failed to build input stream");

    stream.play().expect("Failed to play stream");

    loop {
        sleep(Duration::from_secs(3600));
    }
}

/// Keeps the state throughout the app's lifetime.
struct RmsState {
    moving_avg: MovingAverage,
    max_rms: f32,
    min_rms: f32,
    current_brightness: f32,
}

impl Default for RmsState {
    fn default() -> Self {
        Self {
            moving_avg: MovingAverage::new(10),
            max_rms: f32::MIN, // assume initially we want any value to be greater
            min_rms: f32::MAX, // assume initially we want any value to be smaller
            current_brightness: 0.0f32,
        }
    }
}

impl RmsState {
    /// Takes in a RMS value and updates the min and max.
    fn update_rms_min_max(&mut self, value: f32) {
        if value < self.min_rms {
            self.min_rms = value;
        }
        if value > self.max_rms {
            self.max_rms = value;
        }
    }
}

/// A simple moving average calculator for real-time data.
struct MovingAverage {
    /// The number of items to average over.
    size: usize,
    /// The recent values used to compute the average.
    window: VecDeque<f32>,
}

impl MovingAverage {
    fn new(size: usize) -> Self {
        Self {
            size,
            window: VecDeque::new(),
        }
    }

    fn update(&mut self, data: f32) -> &mut Self {
        self.window.push_back(data);
        if self.window.len() > self.size {
            self.window.pop_front();
        }

        self
    }

    fn value(&self) -> f32 {
        if self.window.is_empty() {
            0.0
        } else {
            self.window.iter().sum::<f32>() / self.window.len() as f32
        }
    }
}

/// Taes a chunk of audio data point (always the same length) and updates the keyboard backlights.
fn process_audio_chunk(chunk: &[f32], state: &mut RmsState) {
    let rms = calc_rms(chunk);
    //state.update_rms_min_max(rms);
    state.moving_avg.update(rms);

    let min_rms = state.min_rms;
    let max_rms = state.max_rms;

    //let threshold = (state.moving_avg.value() * 1.5).max(1.0);
    let threshold = state.moving_avg.value() * 1.4;
    let normalized_rms: f32 = if max_rms > min_rms {
        ((rms - min_rms) / (max_rms - min_rms)) * 100.0
    } else {
        0.0
    };

    let boost: f32 = 1.6;

    let boosted = normalized_rms.powf(boost); // 1.0 = linear, >1 = sensitive at low end
    let brightness = (boosted).clamp(0.0, 100.0);

    if rms > threshold {
        state.current_brightness = brightness;
        println!("Changing backlight to: {brightness:.02}!");
        set_brightness(brightness).unwrap();
    } else {
        state.current_brightness -= 1.0;
        set_brightness(state.current_brightness).unwrap();
    }
}

/// Sets the brightness of the keyboard backlight.
fn set_brightness(level: f32) -> io::Result<()> {
    let level_whole: u8 = level as u8;
    let path: &str = "/sys/class/leds/chromeos::kbd_backlight/brightness";

    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(level_whole.to_string().as_bytes())?;
    Ok(())
}

fn calc_rms(data: &[f32]) -> f32 {
    let mean_squares = data.iter().map(|x| x * x).sum::<f32>() / data.len() as f32;
    mean_squares.sqrt()
}
