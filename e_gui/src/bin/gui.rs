use std::{default, time::Duration};

use egui::{Label, Spinner};
use tokio::{
    sync::{self, mpsc},
    time::sleep,
};
use tracing::warn;
use ui::{
    bufrecv::{mpsc_chan, MPSCRecv},
    conn::{find_conn, USBState},
};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        hardware_acceleration: eframe::HardwareAcceleration::Preferred,
        ..Default::default()
    };

    eframe::run_native(
        "control",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            let (usb_sx, usb_rx) = mpsc_chan(ctx, 1);
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(async {
                    loop {
                        let _ = find_conn(usb_sx.clone()).await;
                        warn!("disconnected");
                    }

                    Result::<_, anyhow::Error>::Ok(())
                })
            });

            Ok(Box::new(UIState { usb_conn: usb_rx }))
        }),
    )
}

struct UIState {
    usb_conn: MPSCRecv<USBState>,
}

impl eframe::App for UIState {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if *self.usb_conn.buf_recv() == USBState::Connected {
                    ui.label("connected");
                } else {
                    ui.label("waiting for device");
                    ui.add(Spinner::new());
                }
            });
        });
    }
}
