//! Entry point of the VaultView desktop application.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    vaultview::app::run()
}