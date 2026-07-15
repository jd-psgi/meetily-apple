/**
 * Supported media file extensions for import and retranscription.
 * IMPORTANT: Keep in sync with Rust constant in src-tauri/src/audio/constants.rs
 *
 * Includes:
 * - Audio (native): MP4, M4A, WAV, MP3, FLAC, OGG, AAC
 * - Audio (FFmpeg-backed): MKV, WebM, WMA
 * - Video (audio track extracted via FFmpeg): MOV, M4V, AVI, MPG, MPEG, WMV, FLV, 3GP, TS
 */
export const AUDIO_EXTENSIONS = [
  // Audio
  'mp4', 'm4a', 'wav', 'mp3', 'flac', 'ogg', 'aac', 'mkv', 'webm', 'wma',
  // Video (audio track extracted)
  'mov', 'm4v', 'avi', 'mpg', 'mpeg', 'wmv', 'flv', '3gp', 'ts',
] as const;

export type AudioExtension = typeof AUDIO_EXTENSIONS[number];

export const isAudioExtension = (ext: string): ext is AudioExtension =>{
  return (AUDIO_EXTENSIONS as readonly string[]).includes(ext);
}

/**
 * Human-readable format names for display
 */
export const AUDIO_FORMAT_DISPLAY_NAMES: Record<AudioExtension, string> = {
  mp4: 'MP4',
  m4a: 'M4A',
  wav: 'WAV',
  mp3: 'MP3',
  flac: 'FLAC',
  ogg: 'OGG',
  aac: 'AAC',
  mkv: 'MKV',
  webm: 'WebM',
  wma: 'WMA',
  mov: 'MOV',
  m4v: 'M4V',
  avi: 'AVI',
  mpg: 'MPG',
  mpeg: 'MPEG',
  wmv: 'WMV',
  flv: 'FLV',
  '3gp': '3GP',
  ts: 'TS',
};

/**
 * Get comma-separated list for UI display
 */
export function getAudioFormatsDisplayList(): string {
  return AUDIO_EXTENSIONS.map(ext => AUDIO_FORMAT_DISPLAY_NAMES[ext]).join(', ');
}
