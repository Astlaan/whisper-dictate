# Windows OpenAI Whisper Dictation

A fast, REST-based dictation tool for Windows, utilizing the renowned OpenAI Whisper model for accurate speech-to-text transcription.

## Features

-   **High-Quality Transcription**: Leverages the power of OpenAI's Whisper model.
-   **Simple to Use**: Designed for quick and easy dictation.
-   **System Tray Integration**: Runs discreetly in the system tray.
-   **Global Hotkey**: Start and stop recording from any application.

## Getting Started

### Prerequisites

-   Windows Operating System.
-   An `OPENAI_API_KEY` environment variable set with your valid OpenAI API key.

### Installation & Execution

1.  Download the latest release executable (e.g., `windows-openai-whisper.exe`) from the releases page.
2.  To start the application, simply double-click the executable. It will appear in your system tray.

## How to Use

-   **Toggle Recording**: The global hotkey `CTRL + ALT + SPACE` acts as a toggle. Press it once to start recording your voice; press it again to stop.
-   **Transcription**: After you stop recording, the audio is processed. The transcribed text will then be automatically typed into your currently active window.

## Important Notes

### Maximum Recording Time

The OpenAI Whisper API imposes a file upload limit of 25 MB. With this application's MP3 encoding settings (128 kbps stereo), this allows for a maximum recording duration of approximately **27 minutes**.

*Detailed calculation: The API limit is 25 MB (26,214,400 bytes). The audio is encoded at 128 kbps (16,000 bytes per second). Therefore, the maximum duration is 26,214,400 bytes / 16,000 bytes/second = 1638.4 seconds, which is roughly 27.3 minutes.*

Please keep this recording limit in mind. For dictating longer content, it's advisable to do so in segments.

### Audio Configuration

This tool uses your system's default microphone for audio capture. The captured audio is then encoded into a 128 kbps stereo MP3 format before being sent to the OpenAI Whisper API for transcription.
