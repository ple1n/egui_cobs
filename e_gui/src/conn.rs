use std::{
    future::Future,
    io::{self, Write},
    marker::PhantomData,
    sync::atomic::{AtomicU32, AtomicU64},
};

use anyhow::Result;
use common::{WireMessage, MAX_PACKET_SIZE};
use futures::{
    channel::mpsc::{self, Receiver, Sender},
    SinkExt, StreamExt,
};
use postcard::to_vec_cobs;
use ringbuf::traits::{Consumer, Observer, Producer, RingBuffer};
use ringbuf::{HeapRb, LocalRb};
use serialport::{SerialPort, SerialPortType};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinSet,
};
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, info, trace};

use crate::{
    bufrecv::{EguiSender, Merge},
    Data,
};

#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub enum USBState {
    Connected,
    #[default]
    Waiting,
}

impl Merge for USBState {
    type Delta = Self;
    fn merge(&mut self, incoming: Self::Delta) {
        *self = incoming;
    }
}

pub async fn find_conn(sx: EguiSender<USBState>, data: EguiSender<Data>) -> Result<bool> {
    info!("find conn");
    let ports = serialport::available_ports()?;
    let mut fv: JoinSet<std::result::Result<(), anyhow::Error>> = JoinSet::new();
    let mut found = false;
    for p in ports {
        if let SerialPortType::UsbPort(u) = p.port_type {
            if u.manufacturer.as_ref().unwrap() == "Plein" {
                let (c, f) = make_conn(p.port_name);
                fv.spawn(f);
                fv.spawn(c.meter(data.clone()));
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

pub const BUF_SIZE: usize = MAX_PACKET_SIZE * 2;

pub struct Conn {
    from_dev: Receiver<WireMessage>,
    to_dev: Sender<WireMessage>,
}

fn make_conn(portname: String) -> (Conn, impl Future<Output = Result<()>>) {
    let (sx_from, rx_from) = futures::channel::mpsc::channel(128);
    let (sx_to, rx_to) = futures::channel::mpsc::channel(128);
    (
        Conn {
            from_dev: rx_from,
            to_dev: sx_to,
        },
        handle_conn(portname, sx_from, rx_to),
    )
}

impl Conn {
    pub async fn test(mut self) -> Result<()> {
        self.to_dev.send(WireMessage::default()).await?;
        while let Some(m) = self.from_dev.next().await {
            info!("recv {:?}", &m);
        }
        info!("test fin");
        Ok(())
    }
    pub async fn meter(mut self, sx: EguiSender<Data>) -> Result<()> {
        info!("meter started");
        while let Some(m) = self.from_dev.next().await {
            let _ = sx.send(Data {
                points: m.reports.into_iter().collect(),
            }).await;
        }

        Ok(())
    }
}

async fn handle_conn(
    portname: String,
    mut from_dev: Sender<WireMessage>,
    mut rx: Receiver<WireMessage>,
) -> anyhow::Result<()> {
    info!("conn {}", &portname);
    let mut dev = tokio_serial::new(portname.clone(), 9600).open_native_async()?;

    dev.set_exclusive(false)?;
    use ringbuf::*;
    let mut rbuf: LocalRb<storage::Heap<u8>> = LocalRb::new(BUF_SIZE);
    dev.clear(serialport::ClearBuffer::All)?;
    AsyncWriteExt::flush(&mut dev).await?;

    tokio::try_join!(
        async {
            loop {
                let mut readbuf = Vec::with_capacity(BUF_SIZE);
                let mut packet = Vec::with_capacity(BUF_SIZE);
                let mut terminated = false;
                while !terminated {
                    readbuf.clear();
                    let n = dev.read_buf(&mut readbuf).await?;
                    if n == 0 {
                        anyhow::bail!("device eof")
                    }
                    let k = rbuf.push_slice(&readbuf[..n]);
                    for b in rbuf.pop_iter() {
                        match packet.len() {
                            0 => {
                                if b == 0 {
                                    continue;
                                } else {
                                    packet.push(b)
                                }
                            }
                            _ => {
                                if b == 0 {
                                    packet.push(0);
                                    terminated = true;
                                    break;
                                } else {
                                    packet.push(b);
                                }
                            }
                        }
                    }
                }
                let des: Result<WireMessage, postcard::Error> =
                    postcard::from_bytes_cobs(&mut packet);
                match des {
                    Ok(decoded) => {
                        from_dev.send(decoded).await?;
                    }
                    Err(er) => {
                        println!("read err, {:?}", er);
                    }
                }
            }
            Ok(())
        },
        async {
            let mut dev = tokio_serial::new(portname, 9600).open_native_async()?;
            while let Some(pakt) = rx.next().await {
                let en: heapless::Vec<u8, MAX_PACKET_SIZE> = to_vec_cobs(&pakt).unwrap();
                AsyncWriteExt::write_all(&mut dev, &en).await?;
                AsyncWriteExt::flush(&mut dev).await?;
            }
            Ok(())
        }
    )?;

    Ok(())
}
