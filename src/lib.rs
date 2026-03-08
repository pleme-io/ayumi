//! Ayumi (歩み) — git signs, blame, staging, and diff integration for Neovim.
//!
//! Part of the blnvim-ng distribution — a Rust-native Neovim plugin suite.
//! Built with [`nvim-oxi`](https://github.com/noib3/nvim-oxi) for zero-cost
//! Neovim API bindings.
//!
//! # Features
//!
//! - Sign-column indicators for added, modified, and deleted lines (extmark-based)
//! - Inline git blame as virtual text
//! - Real-time updates on buffer changes and writes
//!
//! # Architecture
//!
//! - [`diff`] — pure Rust parser for unified diff output
//! - [`git`] — subprocess wrappers for `git diff` and `git blame`
//! - [`signs`] — extmark placement and highlight group definitions

pub mod diff;
pub mod git;
pub mod signs;

use nvim_oxi as oxi;
use nvim_oxi::api::Buffer;

use tane::prelude::*;

/// Convert a `tane::Error` into an `oxi::Error` via `api::Error::Other`.
fn tane_err(e: tane::Error) -> oxi::Error {
    oxi::Error::from(oxi::api::Error::Other(e.to_string()))
}

/// Update signs for the current buffer.
///
/// Clears existing extmarks and places fresh ones based on the current
/// diff state against HEAD.
fn update_signs(mut buf: Buffer, ns_id: u32) -> oxi::Result<()> {
    let path = buf.get_name()?;
    if path.as_os_str().is_empty() {
        return Ok(());
    }

    if !git::is_in_repo(&path) {
        return Ok(());
    }

    signs::clear_signs(&mut buf, ns_id)?;

    match git::diff_file(&path) {
        Ok(hunks) => {
            signs::place_signs(&mut buf, ns_id, &hunks)?;
        }
        Err(_) => {
            // Silently ignore git errors (e.g., file not tracked).
        }
    }

    Ok(())
}

#[oxi::plugin]
fn ayumi() -> oxi::Result<()> {
    let ns = Namespace::create("ayumi").map_err(tane_err)?;
    let ns_id = ns.id();

    // Define highlight groups on load.
    signs::define_highlights().map_err(tane_err)?;

    // Update signs when entering a buffer.
    Autocmd::on(&["BufEnter"])
        .group("ayumi")
        .desc("Ayumi: update git signs on buffer enter")
        .register(move |args| {
            let _ = update_signs(args.buffer, ns_id);
            Ok(false)
        })
        .map_err(tane_err)?;

    // Update signs after writing or changing text.
    Autocmd::on(&["BufWritePost", "TextChanged"])
        .group("ayumi")
        .desc("Ayumi: refresh git signs on change")
        .register(move |args| {
            let _ = update_signs(args.buffer, ns_id);
            Ok(false)
        })
        .map_err(tane_err)?;

    Ok(())
}
