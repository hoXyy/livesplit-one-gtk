use clap::Parser;
use mimalloc::MiMalloc;
use relm4::RelmApp;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod app;
mod cli;
mod config;
mod notes;
mod platform;
mod setting_rows;
mod speedrun_com;

#[cfg(feature = "auto-splitting")]
mod autosplitter_registry;

fn main() {
    gio::resources_register_include!("livesplit-one.gresource")
        .expect("failed to register application resources");

    let cli = cli::Cli::parse();
    let config = config::Config::load(cli);
    RelmApp::new("org.livesplit.LiveSplitOne").run::<app::AppModel>(config);
}
