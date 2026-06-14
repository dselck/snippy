use snippy2::ui::MainApp;

fn main() -> Result<(), eframe::Error> {
    // 1. Set up native window options
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Snippy 2")
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([800.0, 500.0]),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(eframe::egui_wgpu::WgpuSetupCreateNew {
                device_descriptor: std::sync::Arc::new(|adapter| {
                    let mut required_features = wgpu::Features::empty();
                    if adapter.features().contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) {
                        required_features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
                    }
                    wgpu::DeviceDescriptor {
                        required_features,
                        ..Default::default()
                    }
                }),
                ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    // 2. Start the eframe window and execute the egui loop
    eframe::run_native(
        "Snippy 2",
        options,
        Box::new(|cc| Ok(Box::new(MainApp::new(cc)))),
    )
}
