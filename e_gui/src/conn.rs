use std::io;

use anyhow::Result;
use common::WireMessage;
use serialport::SerialPortType;
use tokio::task::JoinSet;
use tokio_serial::SerialPortBuilderExt;
use tracing::debug;

use crate::bufrecv::EguiSender;


#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub enum USBState {
    Connected,
    #[default]
    Waiting,
}

pub async fn find_conn(sx: EguiSender<USBState>) -> Result<bool> {
    let ports = serialport::available_ports()?;
    let mut fv: JoinSet<std::result::Result<(), anyhow::Error>> = JoinSet::new();
    fv.spawn(async { Ok(()) });
    let mut found = false;
    for p in ports {
        if let SerialPortType::UsbPort(u) = p.port_type {
            if u.manufacturer.as_ref().unwrap() == "Plein" {
                fv.spawn(handle_conn(p.port_name));
                sx.send(USBState::Connected).await?;
                found = true;
            }
        }
    }

    loop {
        if let Some(rx) = fv.join_next().await {
            rx??;
        } else {
            break;
        }
    }

    Ok(found)
}

pub async fn handle_conn(portname: String) -> Result<()> {
    debug!("handle conn {}", portname);
    let mut dev = tokio_serial::new(portname, 9600)
        .open_native_async()
        .unwrap();

    dev.set_exclusive(true)?;
    // let msg = Message::default();
    // let coded = serde_json::to_vec(&msg)?;
    // dev.write_all(&coded).await?;

    let mut buf = [0; 2048];
    let mut skip = 0;
    loop {
        match dev.try_read(&mut buf[skip..]) {
            Result::Ok(n) => {
                if n > 0 {
                    debug!("read_len={}, {:?}", n, &buf[..10]);
                    let mut copy = buf.clone();
                    match postcard::take_from_bytes_cobs::<WireMessage>(&mut copy[..(n + skip)]) {
                        Ok((decoded, remainder)) => {
                            buf[..remainder.len()].copy_from_slice(&remainder);
                            skip = remainder.len();
                            dbg!(&decoded);
                        }
                        Err(er) => {
                            debug!("read, {:?}", er);
                            let sentinel = buf.iter().position(|k| *k == 0);
                            if let Some(pos) = sentinel {
                                buf[..(copy.len() - pos)].copy_from_slice(&copy[pos..]);
                                skip = 0;
                            } else {
                                buf.fill(0);
                                skip = 0;
                            }
                        }
                    }
                }
            }
            Result::Err(er) => {
                if er.kind() == io::ErrorKind::WouldBlock {
                    dev.readable().await?;
                }
            }
        }
    }
    Ok(())
}
