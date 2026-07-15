// apple_speech_engine/mod.rs
//
// Wraps Apple's on-device SpeechAnalyzer/SpeechTranscriber (macOS 26+) as a
// transcription engine, via the `apple-speech-helper` Swift sidecar (Rust
// can't call these Swift-only APIs directly).

mod apple_speech_engine;
pub mod commands;

pub use apple_speech_engine::AppleSpeechEngine;
