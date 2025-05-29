# Whisper-dictate

A fast, Rust-based dictation tool for Windows, utilizing the renowned OpenAI Whisper model for accurate speech-to-text transcription.

## Features

-   **High-Quality Transcription**: Leverages the power of OpenAI's Whisper model.
-   **Simple to Use**: Designed for quick and easy dictation.
-   **System Tray Integration**: Runs discreetly in the system tray.
-   **Global Hotkey**: Start and stop recording from any application.

## Getting Started

### Prerequisites

-   Windows Operating System.
-   An `OPENAI_API_KEY` environment variable set with your valid OpenAI API key.
-   Enough credits in your OpenAI account. The Whisper API costs $0.004 per minute of audio.

### Installation & Execution

1.  Download the latest release executable (e.g., `windows-openai-whisper.exe`) and `mp3lame.dll` from the releases page.
2.  Ensure both `windows-openai-whisper.exe` and `mp3lame.dll` are in the same directory.
3.  To start the application, simply double-click the executable. It will appear in your system tray.

## How to Use

-   **Toggle Recording**: The global hotkey `CTRL + ALT + SPACE` acts as a toggle. Press it once to start recording your voice; press it again to stop.
-   **Transcription**: After you stop recording, the audio is processed. Please allow a brief moment for the API to transcribe the audio and return the response; processing time is proportional to recording duration. The transcribed text will then be automatically typed into your currently active window. To ensure accurate placement, **avoid moving your cursor while waiting for the response.**

## Important Notes

### Maximum Recording Time

The OpenAI Whisper API imposes a file upload limit of 25 MB. With this application's MP3 encoding settings (128 kbps stereo), this allows for a maximum recording duration of approximately **27 minutes**.

*Detailed calculation: The API limit is 25 MB (26,214,400 bytes). The audio is encoded at 128 kbps (16,000 bytes per second). Therefore, the maximum duration is 26,214,400 bytes / 16,000 bytes/second = 1638.4 seconds, which is roughly 27.3 minutes.*

Please keep this recording limit in mind. For dictating longer content, it's advisable to do so in segments.

### Audio Configuration

This tool uses your system's default microphone for audio capture. The captured audio is then encoded into a 128 kbps stereo MP3 format before being sent to the OpenAI Whisper API for transcription.

### Run at Startup (Windows)

To have Whisper-dictate launch automatically when you start your computer:

1.  **Locate the Executable**: Find the `windows-openai-whisper.exe` file (and `mp3lame.dll`) where you extracted the release.
2.  **Create a Shortcut**: Right-click on `windows-openai-whisper.exe` and select "Create shortcut".
3.  **Open Startup Folder**: Press `Win + R` to open the Run dialog, type `shell:startup`, and press Enter. This will open your Windows Startup folder.
4.  **Move Shortcut**: Drag and drop the newly created shortcut into the Startup folder.

Now, Whisper-dictate will automatically start every time you log in to Windows.
