#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use enigo::Keyboard;
use std::{
    env,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    time::Instant,
};
use tokio::sync::Mutex;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

mod assets;
use anyhow::Result;

mod utils;
use utils::*;

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;
use lame::Lame;
use reqwest::Client;
use tray_icon::{
    menu::{Menu, MenuItem},
    TrayIcon, TrayIconBuilder,
};

struct AppState {
    recording: bool,
    start_time: Option<Instant>,
    tray: Arc<tokio::sync::Mutex<TrayIcon>>,
    pcm_buffer: Option<Arc<std::sync::Mutex<Vec<i16>>>>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
    stream: Option<cpal::Stream>,
    record_flag: AtomicBool,
    processing: bool,
    default_icon: tray_icon::Icon,
    recording_icon: tray_icon::Icon,
}

const ICON_DEFAULT_BYTES: &'static [u8] = include_bytes!("../assets/icon.ico");
const ICON_RECORDING_BYTES: &'static [u8] = include_bytes!("../assets/icon-recording-1.ico");

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| anyhow::anyhow!("COM initialization failed: {:?}", e))?;
    }
    // Create icons here (main thread)
    let icon_rgba_default = ico_to_rgba(ICON_DEFAULT_BYTES).unwrap();
    let default_icon = tray_icon::Icon::from_rgba(
        icon_rgba_default.0,
        icon_rgba_default.1,
        icon_rgba_default.2,
    )
    .unwrap();

    let icon_rgba_recording = ico_to_rgba(ICON_RECORDING_BYTES).unwrap();
    let recording_icon = tray_icon::Icon::from_rgba(
        icon_rgba_recording.0,
        icon_rgba_recording.1,
        icon_rgba_recording.2,
    )
    .unwrap();

    // Load embedded icon data for initial tray icon.
    let menu = Menu::new();
    let exit_item = MenuItem::new("Exit", true, None);
    menu.append(&exit_item).unwrap();

    let tray_item = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(default_icon.clone())
        .build()
        .unwrap();

    let tray_rc = Arc::new(tokio::sync::Mutex::new(tray_item));
    let state = Arc::new(tokio::sync::Mutex::new(AppState {
        recording: false,
        start_time: None,
        tray: tray_rc,
        pcm_buffer: None,
        sample_rate: None,
        channels: None,
        stream: None,
        record_flag: AtomicBool::new(false),
        processing: false,
        default_icon,
        recording_icon,
    }));

    // Global hotkey setup
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(10);
    let tx_clone = tx.clone();

    std::thread::spawn(move || {
        use global_hotkey::GlobalHotKeyEvent;
        use tao::platform::windows::EventLoopBuilderExtWindows;

        let event_loop = tao::event_loop::EventLoopBuilder::<()>::new()
            .with_any_thread(true)
            .build();

        // Create the GlobalHotKeyManager inside this thread
        let manager = GlobalHotKeyManager::new().unwrap();
        manager.register(hotkey).unwrap();

        let receiver = GlobalHotKeyEvent::receiver();

        event_loop.run(move |event, _, control_flow| {
            *control_flow = tao::event_loop::ControlFlow::Poll; // or wait as needed

            // Poll and process hotkey events:
            if let Ok(event) = receiver.try_recv() {
                if event.id == hotkey.id() {
                    // Forward hotkey event, e.g., send via async channel:
                    let _ = tx_clone.blocking_send(());
                }
            }

            // Optionally: Process other OS events if necessary
            match event {
                _ => {}
            }
        });
    });

    // Main event loop waiting for hotkey events
    while let Some(_) = rx.recv().await {
        let mut state_locked = state.lock().await;

        if state_locked.processing {
            // Ignore new hotkey events while we're processing a recording.
            continue;
        }

        if state_locked.processing {
            // Ignore new hotkey events while we're processing a recording.
            continue;
        }

        if !state_locked.recording {
            if let Err(e) = start_recording(&mut state_locked).await {
                eprintln!("Error starting recording: {:?}", e);
            }
        } else {
            // Set processing to true to block further hotkey events.
            state_locked.processing = true;
            drop(state_locked);
            if let Err(e) = stop_and_process(Arc::clone(&state)).await {
                eprintln!("Error processing recording: {:?}", e);
            }
        }
    }

    unsafe {
        CoUninitialize();
    }
    Ok(())
}

async fn start_recording(state: &mut AppState) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(anyhow::anyhow!("No input device"))?;
    let config = device.default_input_config()?;
    println!("Using input device: {:?}", device.name()?);

    // Store the number of channels from the device
    state.channels = Some(config.channels());
    println!("Channels: {:?}", state.channels.unwrap());

    state.pcm_buffer = Some(Arc::new(std::sync::Mutex::new(Vec::<i16>::new())));
    state.sample_rate = Some(config.sample_rate().0 as u32);
    println!("Sample rate: {:?}", state.sample_rate.unwrap());
    let pcm_buffer = state.pcm_buffer.as_ref().unwrap().clone();
    let err_fn = |err| eprintln!("Audio stream error: {:?}", err);

    let stream = match config.sample_format() {
        SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            {
                let pcm_buffer = pcm_buffer.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut buf = pcm_buffer.lock().unwrap();
                    buf.extend_from_slice(data);
                    if buf.len() < 100 {
                        println!(
                            "Captured samples: {:?}",
                            &data[..std::cmp::min(10, data.len())]
                        );
                    }
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            {
                let pcm_buffer = pcm_buffer.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buf = pcm_buffer.lock().unwrap();
                    buf.extend(data.iter().map(|&s| (s * 32767.0) as i16));
                }
            },
            err_fn,
            None,
        )?,
        _ => return Err(anyhow::anyhow!("Unsupported sample format")),
    };
    // Start the stream.
    stream.play()?;
    state.stream = Some(stream);
    state.recording = true;
    state.start_time = Some(Instant::now());
    state
        .tray
        .lock()
        .await
        .set_icon(Some(state.recording_icon.clone()))?;

    // Mark recording in the shared flag.
    state.record_flag.store(true, Ordering::Relaxed);

    Ok(())
}

async fn stop_and_process(state: Arc<Mutex<AppState>>) -> Result<()> {
    let result = (async {
        let mut state_locked = state.lock().await;
        if let Some(stream) = state_locked.stream.take() {
            if let Err(e) = stream.pause() {
                state_locked.record_flag.store(false, Ordering::Relaxed);
                let err_msg = format!("Failed to stop audio: {}", e);
                drop(state_locked);
                show_balloon(state.clone(), "Error", &err_msg).await;
                return Err(anyhow::anyhow!(err_msg));
            }
        }
        state_locked.record_flag.store(false, Ordering::Relaxed);
        drop(state_locked);

        // Set icon back to default and update state immediately after stopping recording.
        let mut state_locked = state.lock().await;
        state_locked
            .tray
            .lock()
            .await
            .set_icon(Some(state_locked.default_icon.clone()))?;
        state_locked.recording = false;
        drop(state_locked); // Drop the lock before the async block

        // Update the tray to indicate processing.
        show_balloon(
            state.clone(),
            "Processing",
            "Processing audio transcription...",
        )
        .await;

        // Retrieve the collected PCM samples.
        let pcm_buffer = {
            let mut state_locked = state.lock().await;
            state_locked
                .pcm_buffer
                .take()
                .ok_or_else(|| anyhow::anyhow!("No PCM buffer available"))?
        };
        let samples: Vec<i16> = {
            let buf = pcm_buffer.lock().unwrap();
            buf.clone()
        };

        // Retrieve the sample rate stored during recording.
        let sample_rate = {
            let state_locked = state.lock().await;
            state_locked
                .sample_rate
                .ok_or_else(|| anyhow::anyhow!("No sample rate available"))?
        };

        // Encode to MP3 using LAME.
        let mut lame = Lame::new().unwrap();
        lame.set_channels(2).unwrap(); // Stereo
        lame.set_sample_rate(sample_rate).unwrap();
        lame.set_quality(2).unwrap(); // Good quality
        lame.set_kilobitrate(128).unwrap(); // 128 kbps
        lame.init_params().unwrap();

        let mut left = Vec::with_capacity(samples.len() / 2);
        let mut right = Vec::with_capacity(samples.len() / 2);
        for chunk in samples.chunks_exact(2) {
            left.push(chunk[0]);
            right.push(chunk[1]);
        }

        let num_samples = left.len(); // samples per channel
        let mp3_buffer_size = (num_samples as f32 * 1.25).ceil() as usize + 7200;
        // Retrieve the channel count that was stored when recording started.
        let channels = {
            let state_locked = state.lock().await;
            state_locked.channels.unwrap_or(2)
        };

        let (left, right) = if channels == 1 {
            // For mono input, duplicate samples for both channels.
            (samples.clone(), samples.clone())
        } else {
            // For input with 2 or more channels, we assume the first two channels are desired.
            let frames = samples.len() / (channels as usize);
            let mut left = Vec::with_capacity(frames);
            let mut right = Vec::with_capacity(frames);
            for i in 0..frames {
                left.push(samples[i * (channels as usize)]);
                right.push(samples[i * (channels as usize) + 1]);
            }
            (left, right)
        };

        let mut mp3_output = vec![0u8; mp3_buffer_size];
        let encoded = lame.encode(&left, &right, &mut mp3_output[..]).unwrap();
        mp3_output.truncate(encoded);

        let mut flush_buffer = vec![0u8; 7200];
        let flushed = lame.encode(&[], &[], &mut flush_buffer[..]).unwrap();
        mp3_output.extend_from_slice(&flush_buffer[..flushed]);

        // Read the API key now instead of at startup.
        let api_key = match env::var("OPENAI_API_KEY") {
            Ok(val) => val,
            Err(_) => {
                show_balloon(
                    state.clone(),
                    "Error",
                    "OPENAI_API_KEY environment variable not set",
                )
                .await;
                return Err(anyhow::anyhow!("Missing API key"));
            }
        };

        // Use reqwest to call the transcription API.
        let client = Client::new();
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(mp3_output)
                    .file_name("recording.mp3")
                    .mime_str("audio/mpeg")
                    .unwrap(),
            )
            .text("model", "whisper-1");

        let response = client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", api_key))
            .multipart(form)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(e) => {
                show_balloon(state.clone(), "API Error", &format!("{}", e)).await;
                return Err(anyhow::anyhow!("API request failed"));
            }
        };

        let response_text = response.text().await?;
        let transcription_text = match serde_json::from_str::<serde_json::Value>(&response_text) {
            Ok(json) => json["text"].as_str().unwrap_or_default().to_string(),
            Err(e) => {
                show_balloon(state.clone(), "API Error", &format!("{}", e)).await;
                return Err(anyhow::anyhow!("Failed to parse response"));
            }
        };

        // Simulate typing directly using Enigo.
        use enigo::{Enigo, Settings};
        let mut enigo = Enigo::new(&Settings::default()).unwrap();
        let _ = enigo.text(&transcription_text);

        Ok(())
    })
    .await;

    let mut state_locked = state.lock().await;
    state_locked.processing = false;
    result
}

async fn show_balloon(state: Arc<tokio::sync::Mutex<AppState>>, title: &str, msg: &str) {
    let state_guard = state.lock().await;
    let tray_guard = state_guard.tray.lock().await;
    let _ = tray_guard.set_tooltip(Some(&format!("{}: {}", title, msg)));
}
