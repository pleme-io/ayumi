//! Ayumi (歩み) — git signs, blame, staging, and diff integration for Neovim
//!
//! Part of the blnvim-ng distribution — a Rust-native Neovim plugin suite.
//! Built with [`nvim-oxi`](https://github.com/noib3/nvim-oxi) for zero-cost
//! Neovim API bindings.

use nvim_oxi as oxi;

#[oxi::plugin]
fn ayumi() -> oxi::Result<()> {
    Ok(())
}
