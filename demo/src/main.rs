use skulpin::LogicalSize;

use std::ffi::CString;

use atelier_legion_demo::DemoApp;
use atelier_legion_demo::daemon;
//use atelier_legion_demo::game;

fn main() {
    // Setup logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Debug)
        .filter_module("tokio_reactor", log::LevelFilter::Info)
        .init();

    // Spawn the daemon in a background thread. This could be a different process, but
    // for simplicity we'll launch it here.
    std::thread::spawn(move || {
        daemon::run();
    });

    {
        let mut asset_manager = atelier_legion_demo::AssetManager::default();
        asset_manager.temp_force_load_asset();
        asset_manager.temp_force_prefab_cook();
    }

    // Build the app and run it
    let example_app = DemoApp::new();
    skulpin::AppBuilder::new()
        .app_name(CString::new("Skulpin Example App").unwrap())
        .use_vulkan_debug_layer(true)
        .logical_size(LogicalSize::new(900.0, 600.0))
        .run(example_app);
}
