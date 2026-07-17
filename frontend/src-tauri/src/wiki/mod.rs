// Wiki export module: writes each completed meeting summary + transcript to a
// markdown file in a user-configured local folder (e.g. an Obsidian vault or
// other folder-based knowledge base).

pub mod preferences;
pub mod export;
pub mod commands;

pub use export::write_meeting_to_wiki;
