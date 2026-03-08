//! Extmark-based sign column rendering.
//!
//! Places signs (via extmarks) in the Neovim sign column to indicate
//! added, modified, and deleted lines.

use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::SetExtmarkOpts;

use tane::highlight::Highlight;

use crate::diff::{ChangeKind, Hunk};

/// Sign column characters for each change kind.
const SIGN_ADD: &str = "\u{2502}";    // │
const SIGN_MODIFY: &str = "\u{2502}"; // │
const SIGN_DELETE: &str = "\u{25bc}"; // ▼

/// Highlight group names.
pub const HL_ADD: &str = "AyumiAdd";
pub const HL_MODIFY: &str = "AyumiModify";
pub const HL_DELETE: &str = "AyumiDelete";
pub const HL_BLAME: &str = "AyumiBlame";

/// Register the default highlight groups.
///
/// Called once during plugin initialization. Users can override these
/// with `:highlight` commands or colorscheme autocommands.
pub fn define_highlights() -> tane::Result<()> {
    Highlight::new(HL_ADD).fg("#a6e3a1").apply()?;
    Highlight::new(HL_MODIFY).fg("#89b4fa").apply()?;
    Highlight::new(HL_DELETE).fg("#f38ba8").apply()?;
    Highlight::new(HL_BLAME).fg("#6c7086").italic().apply()?;
    Ok(())
}

/// Clear all ayumi extmarks from a buffer.
pub fn clear_signs(buf: &mut Buffer, ns_id: u32) -> nvim_oxi::Result<()> {
    buf.clear_namespace(ns_id, ..)?;
    Ok(())
}

/// Place sign-column extmarks for the given hunks.
pub fn place_signs(
    buf: &mut Buffer,
    ns_id: u32,
    hunks: &[Hunk],
) -> nvim_oxi::Result<()> {
    let line_count = buf.line_count()? as usize;

    for hunk in hunks {
        match hunk.kind {
            ChangeKind::Added => {
                for offset in 0..hunk.count {
                    let line = hunk.start + offset;
                    if line == 0 || line > line_count {
                        continue;
                    }
                    let opts = SetExtmarkOpts::builder()
                        .sign_text(SIGN_ADD)
                        .sign_hl_group(HL_ADD)
                        .build();
                    buf.set_extmark(ns_id, line - 1, 0, &opts)?;
                }
            }
            ChangeKind::Modified => {
                for offset in 0..hunk.count {
                    let line = hunk.start + offset;
                    if line == 0 || line > line_count {
                        continue;
                    }
                    let opts = SetExtmarkOpts::builder()
                        .sign_text(SIGN_MODIFY)
                        .sign_hl_group(HL_MODIFY)
                        .build();
                    buf.set_extmark(ns_id, line - 1, 0, &opts)?;
                }
            }
            ChangeKind::Deleted => {
                // Deletion marker: placed on the line *after* which lines were removed.
                // If start == 0 (deleted from the very beginning), place on line 0.
                let line = if hunk.start == 0 { 1 } else { hunk.start };
                if line > line_count {
                    continue;
                }
                let opts = SetExtmarkOpts::builder()
                    .sign_text(SIGN_DELETE)
                    .sign_hl_group(HL_DELETE)
                    .build();
                buf.set_extmark(ns_id, line - 1, 0, &opts)?;
            }
        }
    }

    Ok(())
}

/// Place inline blame virtual text on a specific line.
pub fn place_blame(
    buf: &mut Buffer,
    ns_id: u32,
    line: usize,
    text: &str,
) -> nvim_oxi::Result<u32> {
    use nvim_oxi::api::types::ExtmarkVirtTextPosition;

    let virt_text: Vec<(&str, &str)> = vec![(text, HL_BLAME)];

    let opts = SetExtmarkOpts::builder()
        .virt_text(virt_text)
        .virt_text_pos(ExtmarkVirtTextPosition::Eol)
        .build();

    Ok(buf.set_extmark(ns_id, line, 0, &opts)?)
}
