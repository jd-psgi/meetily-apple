use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, RwLock};

const DEFAULT_LOCALE: &str = "en-US";
const SIDECAR_SAMPLE_RATE: u32 = 16_000;

// ============================================================================
// Sidecar JSON protocol (mirrors apple-speech-helper's main.swift)
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarRequest {
    Init {
        locale: String,
    },
    Transcribe {
        #[serde(rename = "audio_base64")]
        audio_base64: String,
        #[serde(rename = "sample_rate")]
        sample_rate: u32,
    },
    #[allow(dead_code)]
    Ping,
    Shutdown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarResponse {
    Ready {
        locale: String,
    },
    Transcript {
        text: String,
        #[allow(dead_code)]
        is_final: bool,
    },
    #[allow(dead_code)]
    Pong,
    #[allow(dead_code)]
    Goodbye,
    Error {
        message: String,
    },
}

struct SidecarHandle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

// ============================================================================
// AppleSpeechEngine
// ============================================================================

/// Wraps Apple's on-device SpeechAnalyzer/SpeechTranscriber via the
/// apple-speech-helper sidecar. Unlike Whisper/Parakeet there is no model
/// file to download: `ensure_ready` reserves and (if needed) installs the
/// OS-managed locale asset for the requested locale.
pub struct AppleSpeechEngine {
    helper_binary_path: PathBuf,
    sidecar: Mutex<Option<SidecarHandle>>,
    current_locale: RwLock<Option<String>>,
}

impl AppleSpeechEngine {
    pub fn new() -> Result<Self> {
        Ok(Self {
            helper_binary_path: Self::resolve_helper_binary()?,
            sidecar: Mutex::new(None),
            current_locale: RwLock::new(None),
        })
    }

    /// True if this machine is macOS 26 ("Tahoe") or later, the minimum
    /// version for SpeechAnalyzer/SpeechTranscriber. Checked with `sw_vers`
    /// rather than parsing a crate's OS-version string, since the exact
    /// SpeechAnalyzer availability cutoff is an OS fact, not a Rust one.
    pub fn is_available() -> bool {
        if !cfg!(target_os = "macos") {
            return false;
        }

        std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|version| {
                version
                    .trim()
                    .split('.')
                    .next()
                    .and_then(|major| major.parse::<u32>().ok())
            })
            .map(|major| major >= 26)
            .unwrap_or(false)
    }

    fn resolve_helper_binary() -> Result<PathBuf> {
        if let Ok(env_path) = std::env::var("MEETILY_APPLE_SPEECH_HELPER") {
            let path = PathBuf::from(env_path);
            if path.exists() {
                return Ok(path);
            }
        }

        let target_triple = std::env::var("TARGET").unwrap_or_else(|_| {
            #[cfg(target_arch = "aarch64")]
            {
                "aarch64-apple-darwin".to_string()
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                "x86_64-apple-darwin".to_string()
            }
        });
        let binary_name = format!("apple-speech-helper-{}", target_triple);

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let bundled = exe_dir.join(&binary_name);
                if bundled.exists() {
                    return Ok(bundled);
                }
            }
        }

        if let Ok(resource_dir) = std::env::var("RESOURCE_DIR") {
            let bundled = PathBuf::from(resource_dir).join(&binary_name);
            if bundled.exists() {
                return Ok(bundled);
            }
        }

        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let workspace_root = PathBuf::from(&manifest_dir)
                .parent()
                .and_then(|p| p.parent())
                .ok_or_else(|| anyhow!("Failed to determine workspace root"))?
                .to_path_buf();

            for profile in ["release", "debug"] {
                let candidate = workspace_root
                    .join("apple-speech-helper/.build")
                    .join(profile)
                    .join("apple-speech-helper");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }

        Err(anyhow!(
            "apple-speech-helper binary not found. Build with 'cd apple-speech-helper && swift build -c release' or set MEETILY_APPLE_SPEECH_HELPER."
        ))
    }

    async fn ensure_spawned(guard: &mut Option<SidecarHandle>, helper_binary_path: &PathBuf) -> Result<()> {
        if guard.is_some() {
            return Ok(());
        }

        let mut child = tokio::process::Command::new(helper_binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("Failed to spawn apple-speech-helper at {:?}", helper_binary_path))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get apple-speech-helper stdin"))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("Failed to get apple-speech-helper stdout"))?,
        );

        *guard = Some(SidecarHandle { child, stdin, stdout });
        Ok(())
    }

    async fn request(&self, request: SidecarRequest) -> Result<SidecarResponse> {
        let mut guard = self.sidecar.lock().await;
        Self::ensure_spawned(&mut guard, &self.helper_binary_path).await?;

        let handle = guard.as_mut().expect("sidecar was just spawned");

        let line = serde_json::to_string(&request).context("Failed to serialize sidecar request")?;
        handle
            .stdin
            .write_all(line.as_bytes())
            .await
            .context("Failed to write to apple-speech-helper stdin")?;
        handle.stdin.write_all(b"\n").await?;
        handle.stdin.flush().await.context("Failed to flush apple-speech-helper stdin")?;

        let mut response_line = String::new();
        let bytes_read = handle
            .stdout
            .read_line(&mut response_line)
            .await
            .context("Failed to read apple-speech-helper stdout")?;

        if bytes_read == 0 {
            // Sidecar exited; drop the handle so the next call respawns it.
            let mut dead = guard.take();
            if let Some(handle) = dead.as_mut() {
                let _ = handle.child.kill().await;
            }
            return Err(anyhow!("apple-speech-helper closed its output (it may have crashed)"));
        }

        serde_json::from_str(response_line.trim())
            .with_context(|| format!("Failed to parse apple-speech-helper response: {}", response_line.trim()))
    }

    /// Reserve (and install, if not already present) the on-device asset for
    /// `locale`, defaulting to en-US. Must succeed before `transcribe_audio`.
    pub async fn ensure_ready(&self, locale: Option<String>) -> Result<String> {
        let requested = locale.unwrap_or_else(|| DEFAULT_LOCALE.to_string());

        match self.request(SidecarRequest::Init { locale: requested }).await? {
            SidecarResponse::Ready { locale } => {
                *self.current_locale.write().await = Some(locale.clone());
                Ok(locale)
            }
            SidecarResponse::Error { message } => Err(anyhow!(message)),
            other => Err(anyhow!("Unexpected response to init: {:?}", other)),
        }
    }

    pub async fn is_model_loaded(&self) -> bool {
        self.current_locale.read().await.is_some()
    }

    pub async fn get_current_model(&self) -> Option<String> {
        self.current_locale.read().await.clone()
    }

    pub async fn transcribe_audio(&self, audio_data: Vec<f32>) -> Result<String> {
        if self.get_current_model().await.is_none() {
            return Err(anyhow!(
                "Apple Speech engine has no locale reserved yet; call ensure_ready() first"
            ));
        }

        let raw_bytes: Vec<u8> = audio_data.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        let audio_base64 = base64::engine::general_purpose::STANDARD.encode(raw_bytes);

        match self
            .request(SidecarRequest::Transcribe {
                audio_base64,
                sample_rate: SIDECAR_SAMPLE_RATE,
            })
            .await?
        {
            SidecarResponse::Transcript { text, .. } => Ok(text),
            SidecarResponse::Error { message } => Err(anyhow!(message)),
            other => Err(anyhow!("Unexpected response to transcribe: {:?}", other)),
        }
    }

    /// Gracefully stop the sidecar. Safe to call even if it was never spawned.
    pub async fn shutdown(&self) {
        let mut guard = self.sidecar.lock().await;
        if let Some(handle) = guard.as_mut() {
            let _ = self::send_shutdown(handle).await;
            let _ = handle.child.kill().await;
        }
        *guard = None;
    }
}

async fn send_shutdown(handle: &mut SidecarHandle) -> Result<()> {
    let line = serde_json::to_string(&SidecarRequest::Shutdown)?;
    handle.stdin.write_all(line.as_bytes()).await?;
    handle.stdin.write_all(b"\n").await?;
    handle.stdin.flush().await?;
    Ok(())
}
