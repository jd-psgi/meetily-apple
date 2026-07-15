/// Supported media file extensions for import and retranscription.
///
/// Audio: native Symphonia formats (MP4, M4A, WAV, MP3, FLAC, OGG, AAC) and
/// FFmpeg-backed formats (MKV, WebM, WMA).
///
/// Video: common containers whose audio track is extracted via FFmpeg (`-vn`)
/// during decoding. The video stream is discarded; only audio is transcribed.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    // Audio
    "mp4", "m4a", "wav", "mp3", "flac", "ogg", "aac", "mkv", "webm", "wma",
    // Video (audio track extracted via ffmpeg)
    "mov", "m4v", "avi", "mpg", "mpeg", "wmv", "flv", "3gp", "ts",
];
