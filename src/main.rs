#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use enigo::Keyboard;
use std::{
    env,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    time::Instant,
};
use std::rc::Rc;
use std::cell::RefCell;
// tokio::runtime::Runtime import removed as it's unused. Builder is used directly.
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

mod assets;
use anyhow::Result;

mod utils;
use utils::*;

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager, GlobalHotKeyEvent, HotKeyState, // Added HotKeyState
};
use lame::Lame;
use reqwest::Client;
use tray_icon::{
    menu::{Menu, MenuItem, MenuEvent}, // Added MenuEvent
    TrayIcon, TrayIconBuilder,
};

// Tao imports for the main event loop
use tao::event_loop::{ControlFlow, EventLoop};
// MenuId type might be inferred or directly from tray_icon::menu::MenuItem::id()
// once_cell::sync::Lazy is removed

// No more static EXIT_ITEM_ID

struct AppState {
    recording: bool,
    start_time: Option<Instant>,
    tray: Rc<RefCell<TrayIcon>>, // TrayIcon wrapped for main-thread shared mutability
    pcm_buffer: Option<Arc<std::sync::Mutex<Vec<i16>>>>, // std::sync::Mutex for cpal callback
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

// Main function is no longer async
fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| anyhow::anyhow!("COM initialization failed: {:?}", e))?;
    }

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

    let menu = Menu::new();
    let exit_item = MenuItem::new("Exit", true, None);
    // Assuming id() returns &MenuId and MenuId is Clone (u32 is Clone)
    // Or if id() returns MenuId (u32, which is Copy), .clone() is a no-op or not needed.
    // Given previous compiler error E0507 fix, .id().clone() was accepted.
    let actual_exit_item_id = exit_item.id().clone(); 
    menu.append(&exit_item).unwrap();

    let tray_item = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(default_icon.clone())
        .build()
        .unwrap();

    let tray_rc = Rc::new(RefCell::new(tray_item));

    let state = Rc::new(RefCell::new(AppState {
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

    let event_loop = EventLoop::new();
    let manager = GlobalHotKeyManager::new().unwrap();
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    let hotkey_id = hotkey.id();
    manager.register(hotkey).unwrap();

    // tokio::runtime::Runtime::new() was removed as unused.
    let rt_main = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let state_clone_for_loop = Rc::clone(&state);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        if let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            if menu_event.id == actual_exit_item_id { // Compare with the ID of the actual menu item
                *control_flow = ControlFlow::Exit;
                return;
            }
        }

        if let Ok(ghk_event) = GlobalHotKeyEvent::receiver().try_recv() {
            if ghk_event.id == hotkey_id && ghk_event.state == HotKeyState::Pressed {
                let current_state_rc_clone = Rc::clone(&state_clone_for_loop);
                rt_main.block_on(async move {
                    let (is_processing, is_recording) = {
                        let state_borrow = current_state_rc_clone.borrow();
                        (state_borrow.processing, state_borrow.recording)
                    };

                    if is_processing {
                        return;
                    }

                    if !is_recording {
                        if let Err(e) = start_recording_wrapper(Rc::clone(&current_state_rc_clone)).await {
                            eprintln!("Error starting recording: {:?}", e);
                            show_balloon(Rc::clone(&current_state_rc_clone), "Error", &format!("Start recording failed: {}", e)).await;
                        }
                    } else {
                        { // Scope to set processing to true
                            let mut state_borrow_mut = current_state_rc_clone.borrow_mut();
                            state_borrow_mut.processing = true;
                        } // RefMut guard dropped here
                        if let Err(e) = stop_and_process(Rc::clone(&current_state_rc_clone)).await {
                            eprintln!("Error processing recording: {:?}", e);
                            show_balloon(Rc::clone(&current_state_rc_clone), "Error", &format!("Processing failed: {}", e)).await;
                        }
                    }
                });
            }
        }

        match event {
            tao::event::Event::LoopDestroyed => {
                // Cleanup if necessary
            }
            _ => {}
        }
    });

    unsafe {
        CoUninitialize();
    }
    Ok(())
}

// Wrapper to adapt start_recording to the Rc<RefCell<AppState>> pattern
async fn start_recording_wrapper(state_rc: Rc<RefCell<AppState>>) -> Result<()> {
    let mut state_guard = state_rc.borrow_mut();
    
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(anyhow::anyhow!("No input device"))?;
    let config = device.default_input_config()?;
    println!("Using input device: {:?}", device.name()?);

    state_guard.channels = Some(config.channels());
    println!("Channels: {:?}", state_guard.channels.unwrap());

    state_guard.pcm_buffer = Some(Arc::new(std::sync::Mutex::new(Vec::<i16>::new())));
    state_guard.sample_rate = Some(config.sample_rate().0 as u32);
    println!("Sample rate: {:?}", state_guard.sample_rate.unwrap());
    let pcm_buffer_arc = state_guard.pcm_buffer.as_ref().unwrap().clone();
    let err_fn = |err| eprintln!("Audio stream error: {:?}", err);

    let stream = match config.sample_format() {
        SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            {
                let pcm_buffer_clone = pcm_buffer_arc.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut buf = pcm_buffer_clone.lock().unwrap();
                    buf.extend_from_slice(data);
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            {
                let pcm_buffer_clone = pcm_buffer_arc.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buf = pcm_buffer_clone.lock().unwrap();
                    buf.extend(data.iter().map(|&s| (s * 32767.0) as i16));
                }
            },
            err_fn,
            None,
        )?,
        _ => return Err(anyhow::anyhow!("Unsupported sample format")),
    };
    stream.play()?;
    state_guard.stream = Some(stream);
    state_guard.recording = true;
    state_guard.start_time = Some(Instant::now());
    
    // Access tray via Rc<RefCell<TrayIcon>>
    let tray_clone = Rc::clone(&state_guard.tray);
    tray_clone.borrow_mut().set_icon(Some(state_guard.recording_icon.clone()))?;
    
    state_guard.record_flag.store(true, Ordering::Relaxed);
    Ok(())
}


async fn stop_and_process(state_rc: Rc<RefCell<AppState>>) -> Result<()> {
    let processing_start_time = Instant::now();
    let result = (async {
        // Initial lock to stop stream and update immediate state
        let pcm_buffer_option;
        let sample_rate_option;
        let channels_option;
        let default_icon_clone;
        let recording_duration;
        {
            let mut state_guard = state_rc.borrow_mut();
            if let Some(stream) = state_guard.stream.take() {
                if let Err(e) = stream.pause() {
                    state_guard.record_flag.store(false, Ordering::Relaxed);
                    let err_msg = format!("Failed to stop audio: {}", e);
                    // show_balloon needs its own borrow
                    drop(state_guard);
                    show_balloon(Rc::clone(&state_rc), "Error", &err_msg).await;
                    return Err(anyhow::anyhow!(err_msg));
                }
            }
            state_guard.record_flag.store(false, Ordering::Relaxed);
            
            default_icon_clone = state_guard.default_icon.clone();
            let tray_clone = Rc::clone(&state_guard.tray);
            tray_clone.borrow_mut().set_icon(Some(default_icon_clone.clone()))?;
            state_guard.recording = false;
            
            if let Some(start_time) = state_guard.start_time.take() {
                recording_duration = start_time.elapsed();
                println!("Recording duration: {:?}", recording_duration);
            } else {
                recording_duration = std::time::Duration::new(0, 0);
            }

            pcm_buffer_option = state_guard.pcm_buffer.take();
            sample_rate_option = state_guard.sample_rate;
            channels_option = state_guard.channels;
        } // state_guard (RefMut) is dropped here

        show_balloon(
            Rc::clone(&state_rc), // Pass Rc for show_balloon
            "Processing",
            "Processing audio transcription...",
        )
        .await;

        let pcm_buffer_arc = pcm_buffer_option.ok_or_else(|| anyhow::anyhow!("No PCM buffer available"))?;
        let samples: Vec<i16> = {
            let buf = pcm_buffer_arc.lock().unwrap();
            buf.clone()
        };
        let sample_rate = sample_rate_option.ok_or_else(|| anyhow::anyhow!("No sample rate available"))?;
        let channels = channels_option.unwrap_or(2);

        let mut lame = Lame::new().unwrap();
        lame.set_channels(2).unwrap(); 
        lame.set_sample_rate(sample_rate).unwrap();
        lame.set_quality(2).unwrap(); 
        lame.set_kilobitrate(128).unwrap(); 
        lame.init_params().unwrap();

        let (left, right) = if channels == 1 {
            (samples.clone(), samples.clone())
        } else {
            let frames = samples.len() / (channels as usize);
            let mut left = Vec::with_capacity(frames);
            let mut right = Vec::with_capacity(frames);
            for i in 0..frames {
                left.push(samples[i * (channels as usize)]);
                right.push(samples[i * (channels as usize) + 1]);
            }
            (left, right)
        };
        
        let encoding_start_time = Instant::now();
        let num_samples_per_channel = left.len();
        let mp3_buffer_size = (num_samples_per_channel as f32 * 1.25).ceil() as usize + 7200;
        let mut mp3_output = vec![0u8; mp3_buffer_size];
        let encoded_bytes = lame.encode(&left, &right, &mut mp3_output[..]).unwrap();
        mp3_output.truncate(encoded_bytes);
        println!("Encoded MP3 size: {:.2} KB", encoded_bytes as f64 / 1024.0);

        let mut flush_buffer = vec![0u8; 7200];
        let flushed = lame.encode(&[], &[], &mut flush_buffer[..]).unwrap(); // Reverted to original flush
        mp3_output.extend_from_slice(&flush_buffer[..flushed]);
        let encoding_duration = encoding_start_time.elapsed();
        println!("Encoding duration: {:?}", encoding_duration);

        let api_key = match env::var("OPENAI_API_KEY") {
            Ok(val) => val,
            Err(_) => {
                // show_balloon needs Rc<RefCell<AppState>>
                show_balloon(
                    Rc::clone(&state_rc),
                    "Error",
                    "OPENAI_API_KEY environment variable not set",
                )
                .await;
                return Err(anyhow::anyhow!("Missing API key"));
            }
        };

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
                show_balloon(Rc::clone(&state_rc), "API Error", &format!("{}", e)).await;
                return Err(anyhow::anyhow!("API request failed: {}", e));
            }
        };
        
        let status = response.status();
        let response_text = response.text().await?; // Consume response_data to get text

        if !status.is_success() {
            show_balloon(Rc::clone(&state_rc), "API Error", &format!("API Error {}: {}", status, response_text)).await;
            return Err(anyhow::anyhow!("API request failed with status {}: {}", status, response_text));
        }

        // Now use response_text for parsing JSON
        let transcription_text = match serde_json::from_str::<serde_json::Value>(&response_text) {
            Ok(json) => json["text"].as_str().unwrap_or_default().to_string(),
            Err(e) => {
                show_balloon(Rc::clone(&state_rc), "API Error", &format!("Failed to parse API response: {}", e)).await;
                return Err(anyhow::anyhow!("Failed to parse response: {}", e));
            }
        };

        use enigo::{Enigo, Settings};
        let mut enigo = Enigo::new(&Settings::default()).unwrap();
        let _ = enigo.text(&transcription_text);

        Ok(())
    })
    .await;
    
    // Final lock to set processing to false
    {
        let mut state_guard = state_rc.borrow_mut();
        state_guard.processing = false;
    }
    let processing_duration = processing_start_time.elapsed();
    println!("Total processing duration: {:?}", processing_duration);
    result
}

async fn show_balloon(state_rc: Rc<RefCell<AppState>>, title: &str, msg: &str) {
    let state_guard = state_rc.borrow(); // .borrow() is enough if only accessing tray
    let tray_clone = Rc::clone(&state_guard.tray);
    let tray_guard = tray_clone.borrow(); // .borrow() for TrayIcon
    // Using set_tooltip as a simple way to show feedback.
    // For actual balloon notifications, tray-icon might need specific platform features or another crate.
    // The current set_tooltip will update the hover text.
    let _ = tray_guard.set_tooltip(Some(&format!("{}: {}", title, msg)));
    // Consider adding a timed reset for the tooltip or using a dedicated notification crate if true balloons are needed.
    println!("Balloon: {} - {}", title, msg); // Also print to console for visibility
}
