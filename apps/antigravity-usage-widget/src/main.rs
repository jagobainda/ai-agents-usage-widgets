#![windows_subsystem = "windows"]

mod cli;
mod config;
mod provider;
mod usage;

fn main() {
    std::process::exit(widget_common::run_widget(
        Box::new(provider::AntigravityProvider::new()),
        &config::WIDGET_CONFIG,
    ));
}
