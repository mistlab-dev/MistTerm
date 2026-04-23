use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        ..Default::default()
    };

    eframe::run_native(
        "Test GUI",
        options,
        Box::new(|_cc| Box::new(TestApp)),
    )
}

struct TestApp;

impl eframe::App for TestApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Hello from egui!");
            ui.label("If you can see this, the GUI works!");
        });
    }
}
