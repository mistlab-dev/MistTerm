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
    // #207 S2：0.34+ 拆 logic（状态）+ ui（绘制）；此 demo 无状态，logic 留空。
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {}

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Hello from egui!");
            ui.label("If you can see this, the GUI works!");
        });
    }
}
