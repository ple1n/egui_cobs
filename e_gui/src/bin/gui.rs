use std::{default, time::Duration};

use egui::{widgets, Color32, Label, Spinner};
use egui_plot::{Legend, Line, PlotPoint};
use tokio::{
    sync::{self, mpsc},
    time::sleep,
};
use tracing::warn;
use ui::{
    bufrecv::{mpsc_chan, MPSCRecv},
    conn::{find_conn, USBState},
    Data,
};

use backoff_rs::ExponentialBackoffBuilder;

fn main() -> eframe::Result {
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        hardware_acceleration: eframe::HardwareAcceleration::Preferred,
        ..Default::default()
    };

    let bf = ExponentialBackoffBuilder::default().build();

    eframe::run_native(
        "control",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            let (usb_sx, usb_rx) = mpsc_chan(ctx.clone(), 1);
            let (datasx, datarx) = mpsc_chan(ctx, 1);
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(async {
                    for ix in 0..usize::MAX {
                        let e = find_conn(usb_sx.clone(), datasx.clone()).await;
                        tracing::error!("{:?}", e);
                        let nex = bf.duration(ix);
                        warn!("all devices disconnected. reconnect after {:?}", &nex);
                        sleep(nex).await;
                    }

                    Result::<_, anyhow::Error>::Ok(())
                })
            });

            Ok(Box::new(UIState {
                usb_conn: usb_rx,
                data: datarx,
            }))
        }),
    )
}

struct UIState {
    usb_conn: MPSCRecv<USBState>,
    data: MPSCRecv<Data>,
}

impl eframe::App for UIState {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if *self.usb_conn.buf_recv() == USBState::Connected {
                    let plot = egui_plot::Plot::new("lines").legend(Legend::default());
                    plot.show_axes(true).show_grid(true).show(ui, |ui| {
                        let ix: Vec<_> = self
                            .data
                            .buf_recv()
                            .points
                            .iter()
                            .enumerate()
                            .map(|(ix, particle)| {
                                let k = [ix as f64, particle.particle.pm1 as f64];
                                k
                            })
                            .collect();
                        let line1 = Line::new(ix).name("PM1");

                        ui.line(line1);

                        let ix: Vec<_> = self
                            .data
                            .buf_recv()
                            .points
                            .iter()
                            .enumerate()
                            .map(|(ix, particle)| {
                                let k = [ix as f64, particle.particle.pm2 as f64];
                                k
                            })
                            .collect();
                        let line1 = Line::new(ix).name("PM2");
                        ui.line(line1);

                        let ix: Vec<_> = self
                            .data
                            .buf_recv()
                            .points
                            .iter()
                            .enumerate()
                            .map(|(ix, particle)| {
                                let k = [ix as f64, particle.particle.pm10 as f64];
                                k
                            })
                            .collect();
                        let line1 = Line::new(ix).color(Color32::ORANGE).name("PM10");
                        ui.line(line1);
                    });
                } else {
                    ui.label("waiting for device");
                    ui.add(Spinner::new());
                }
            });
        });
    }
}
