use crate::audio::audio_processing::sanitize_filename;
use crate::database::models::MeetingModel;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::summary::SummaryProcessesRepository;
use crate::state::AppState;
use crate::wiki::preferences::{ensure_wiki_directory, load_wiki_preferences};
use anyhow::{anyhow, Result};
use log::{info, warn};
use tauri::{AppHandle, Manager, Runtime};

/// Write a meeting's summary + transcript to the configured wiki folder.
/// No-op if the wiki feature is disabled or the meeting has no summary yet.
/// Best-effort: failures are logged and swallowed - a wiki-write problem
/// must never affect normal summary saving.
pub async fn write_meeting_to_wiki<R: Runtime>(app: &AppHandle<R>, meeting_id: &str) {
    if let Err(e) = try_write_meeting_to_wiki(app, meeting_id).await {
        warn!("Wiki export skipped for meeting {}: {}", meeting_id, e);
    }
}

async fn try_write_meeting_to_wiki<R: Runtime>(app: &AppHandle<R>, meeting_id: &str) -> Result<()> {
    let prefs = load_wiki_preferences(app).await?;
    if !prefs.enabled {
        return Ok(());
    }

    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;
    let pool = state.db_manager.pool();

    let meeting: MeetingModel = sqlx::query_as("SELECT * FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow!("Meeting not found"))?;

    let summary_markdown = fetch_summary_markdown(pool, meeting_id).await?;

    let (transcript_body, duration_minutes) =
        fetch_transcript_body(pool, meeting_id).await?;

    let attendees = extract_attendees(&summary_markdown);

    ensure_wiki_directory(&prefs.wiki_folder)?;

    let date = meeting.created_at.0.format("%Y-%m-%d").to_string();
    let slug = sanitize_filename(&meeting.title).replace(' ', "-");
    let short_id = meeting_id
        .rsplit('-')
        .next()
        .unwrap_or(meeting_id)
        .chars()
        .take(8)
        .collect::<String>();
    let filename = format!("{}_{}-{}.md", date, slug, short_id);
    let file_path = prefs.wiki_folder.join(&filename);

    let markdown = render_wiki_markdown(
        meeting_id,
        &meeting.title,
        &date,
        duration_minutes,
        &attendees,
        &summary_markdown,
        &transcript_body,
    );

    std::fs::write(&file_path, markdown)?;
    info!("Wrote meeting {} to wiki: {:?}", meeting_id, file_path);

    Ok(())
}

/// Extract the markdown summary body from the stored summary_processes.result JSON.
/// Only the current `{ markdown, summary_json }` shape is supported - the legacy
/// typed shape (key_points/action_items/decisions/main_topics) predates the
/// current markdown-based summary pipeline and isn't worth rendering here.
async fn fetch_summary_markdown(pool: &sqlx::SqlitePool, meeting_id: &str) -> Result<String> {
    let process = SummaryProcessesRepository::get_summary_data(pool, meeting_id)
        .await?
        .ok_or_else(|| anyhow!("No summary process found"))?;

    let result_str = process
        .result
        .ok_or_else(|| anyhow!("No summary result yet"))?;

    let result: serde_json::Value = serde_json::from_str(&result_str)?;
    let markdown = result
        .get("markdown")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Summary result has no markdown field"))?;

    if markdown.trim().is_empty() {
        return Err(anyhow!("Summary markdown is empty"));
    }

    Ok(markdown.to_string())
}

/// Fetch every transcript segment for the meeting (ordered by audio_start_time,
/// same ordering the app already uses for display) and render it as timestamped
/// lines. Also returns the approximate total duration in minutes, derived from
/// the transcript timing rather than a separate metadata file that may not exist.
async fn fetch_transcript_body(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<(String, Option<f64>)> {
    let (transcripts, _total) =
        MeetingsRepository::get_meeting_transcripts_paginated(pool, meeting_id, i64::MAX, 0)
            .await?;

    let mut lines = Vec::with_capacity(transcripts.len());
    let mut max_end_time: Option<f64> = None;

    for t in &transcripts {
        let text = t.transcript.trim();
        if text.is_empty() {
            continue;
        }

        let stamp = match t.audio_start_time {
            Some(seconds) => format_timestamp(seconds),
            None => t.timestamp.clone(),
        };
        lines.push(format!("[{}] {}", stamp, text));

        if let Some(end) = t.audio_end_time {
            max_end_time = Some(max_end_time.map_or(end, |m: f64| m.max(end)));
        }
    }

    let duration_minutes = max_end_time.map(|s| (s / 60.0 * 10.0).round() / 10.0);
    Ok((lines.join("\n"), duration_minutes))
}

fn format_timestamp(seconds: f64) -> String {
    let total_seconds = seconds.max(0.0) as u64;
    let h = total_seconds / 3600;
    let m = (total_seconds % 3600) / 60;
    let s = total_seconds % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

/// Best-effort: pull an "Attendees" list out of the generated summary markdown,
/// if the template used had that section. Never invents names - omitted when
/// no such section is found.
fn extract_attendees(summary_markdown: &str) -> Vec<String> {
    let mut attendees = Vec::new();
    let mut in_section = false;

    for line in summary_markdown.lines() {
        let trimmed = line.trim();
        let heading = trimmed.trim_start_matches('#').trim_start_matches("**").trim_end_matches("**").trim();

        if trimmed.starts_with('#') || (trimmed.starts_with("**") && trimmed.ends_with("**")) {
            in_section = heading.eq_ignore_ascii_case("attendees");
            continue;
        }

        if in_section {
            if let Some(item) = trimmed.strip_prefix('-').or_else(|| trimmed.strip_prefix('*')) {
                let name = item.trim();
                if !name.is_empty() {
                    attendees.push(name.to_string());
                }
            } else if trimmed.is_empty() {
                continue;
            } else {
                break;
            }
        }
    }

    attendees
}

fn render_wiki_markdown(
    meeting_id: &str,
    title: &str,
    date: &str,
    duration_minutes: Option<f64>,
    attendees: &[String],
    summary_markdown: &str,
    transcript_body: &str,
) -> String {
    let mut frontmatter = vec![
        format!("meetily_meeting_id: {}", meeting_id),
        format!("date: {}", date),
    ];
    if let Some(minutes) = duration_minutes {
        frontmatter.push(format!("duration_minutes: {}", minutes));
    }
    if !attendees.is_empty() {
        let list = attendees
            .iter()
            .map(|a| format!("\"{}\"", a.replace('"', "'")))
            .collect::<Vec<_>>()
            .join(", ");
        frontmatter.push(format!("attendees: [{}]", list));
    }
    frontmatter.push("source: meetily".to_string());

    format!(
        "---\n{}\n---\n\n# {}\n\n## Summary\n{}\n\n## Full Transcript\n{}\n",
        frontmatter.join("\n"),
        title,
        summary_markdown.trim(),
        transcript_body.trim(),
    )
}
