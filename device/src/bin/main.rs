#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(iter_next_chunk)]
#![feature(iter_array_chunks)]
#![feature(async_closure)]
#![allow(unreachable_code)]
#![allow(unused_mut)]

use core::{
    borrow::BorrowMut,
    cmp::{max, min},
    future::{pending, Pending},
    hash::{Hash, Hasher, SipHasher},
    mem::MaybeUninit,
    ops::{Range, RangeFrom},
    pin::pin,
    sync::atomic::{self, AtomicBool, AtomicU16, AtomicU32, Ordering::SeqCst},
};

use ::atomic::Atomic;
use bbqueue::BBBuffer;
use bittle::{BitsMut, BitsOwned};
use common::{
    cob::{
        self,
        COBErr::{Codec, NextRead},
        COBSeek, COB,
    },
    Command, Report, WireMessage, MAX_PACKET_SIZE,
};
use defmt::*;
use embassy_futures::{
    join,
    select::{self, Either},
};
use embassy_sync::{
    blocking_mutex::{raw::CriticalSectionRawMutex, CriticalSectionMutex},
    channel::{self, Channel},
    mutex::Mutex,
    pubsub::PubSubChannel,
    signal::Signal,
};
use embassy_usb::{
    class::cdc_acm::{self, CdcAcmClass, State},
    driver::EndpointError,
    Builder, UsbDevice,
};

const QUEUE_LEN: usize = 2;
const RING_BUFLEN: usize = 2;

use futures_util::{future::select, Stream, StreamExt};
use heapless::{Deque, LinearMap, Vec};
use lock_free_static::lock_free_static;
#[cfg(not(feature = "defmt"))]
use panic_halt as _;
use ringbuf::{
    storage::Owning,
    traits::{Consumer, Producer, SplitRef},
    wrap::caching::Caching,
    SharedRb, StaticRb,
};
use static_cell::StaticCell;
#[cfg(feature = "defmt")]
use {defmt_rtt as _, panic_probe as _};

use defmt::*;
use embassy_executor::{SpawnToken, Spawner};
use embassy_stm32::{
    adc::{self, Adc, Temperature},
    bind_interrupts,
    dma::WritableRingBuffer,
    flash::BANK1_REGION,
    gpio::{self, Input, Level, Output, Pull, Speed},
    i2c,
    interrupt::{self, InterruptExt},
    time::{khz, mhz, Hertz},
    timer::{low_level, pwm_input::PwmInput},
    usb::Driver,
    Config, Peripheral,
};
use embassy_stm32::{peripherals, usb};
use embassy_time::{Delay, Duration, Instant, Ticker, Timer};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    USB_LP_CAN1_RX0 => usb::InterruptHandler<peripherals::USB>;
});

type UsbDriver = embassy_stm32::usb::Driver<'static, embassy_stm32::peripherals::USB>;

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;

        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll = Some(Pll {
            src: PllSource::HSE,
            prediv: PllPreDiv::DIV1,
            mul: PllMul::MUL9,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV1;
    }
    let mut p = embassy_stm32::init(config);

    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Plein");
    config.product = Some("Meter");
    config.serial_number = Some("1");

    {
        let _dp = Output::new(&mut p.PA12, Level::Low, Speed::Low);
        Timer::after_millis(10).await;
    }

    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);

    static CONF_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CTRLBUF: StaticCell<[u8; 128]> = StaticCell::new();

    static STATE: StaticCell<State> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut CONF_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [], // no msos descriptors
        &mut CTRLBUF.init([0; 128])[..],
    );

    let class =
        embassy_usb::class::cdc_acm::CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);
    let usb = builder.build();

    unwrap!(spawner.spawn(usb_task(usb)));

    let (mut sx, mut rx) = class.split();
    info!("usb max packet size, {}", sx.max_packet_size());
    let (q_prod, cons) = unwrap!(QUEUE.try_split().map_err(|_| "bbq split"));

    defmt::assert!(RING_BUF.init());

    let (rb_prod, rb_consumer) = unwrap!(RING_BUF.get_mut()).split_ref();
    let mut bufrd = BufReaders {
        queue: cons,
        ringbuf: rb_consumer,
    };
    let sending = Sending {
        queue: q_prod,
        ringbuf: rb_prod,
    };

    let report_fut = async {
        let sig = unwrap!(unsafe { USB_SIGNAL.publisher() });
        loop {
            info!("waiting for usb");
            sx.wait_connection().await;
            unsafe {
                USB_STATE = UsbState::Connected;
                sig.publish(USB_STATE).await;
            }
            info!("Connected");
            let x = join::join(report(&mut sx, &mut bufrd), listen(&mut rx)).await;
            info!("Disconnected, {:?}", x);
            unsafe {
                USB_STATE = UsbState::Waiting;
                sig.publish(USB_STATE).await;
            }
        }
    };

    report_fut.await
}

static USB_SIGNAL: PubSubChannel<CriticalSectionRawMutex, UsbState, 2, 2, 1> = PubSubChannel::new();
static mut USB_STATE: UsbState = UsbState::Waiting;

#[derive(PartialEq, Eq, Clone, Copy)]
enum UsbState {
    Waiting,
    Connected,
}

#[derive(Format)]
struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => crate::panic!("buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

async fn listen<'d, T: 'd + embassy_stm32::usb::Instance>(
    rx: &mut cdc_acm::Receiver<'d, Driver<'d, T>>,
) -> Result<(), Disconnected> {
    info!("listening for commands");
    unsafe {
        let mut usb = unwrap!(USB_SIGNAL.subscriber());
        loop {
            if USB_STATE == UsbState::Connected
                || usb.next_message_pure().await == UsbState::Connected
            {
                break;
            }
        }
    }
    const BUFLEN: usize = MAX_PACKET_SIZE;
    let mut buf = [0; BUFLEN];
    let mut rg: Option<RangeFrom<usize>> = None;

    loop {
        rg.get_or_insert(0..);
        let rd = rx.read_packet(&mut buf[unwrap!(rg.take())]).await?;
        if rd == 0 {
            Timer::after_secs(2).await;
            continue;
        }
        let copy = buf.clone();
        let cob: COB<WireMessage> = COB::new(&mut buf[..], &copy);
        for msg in cob {
            if let Ok(msg) = msg {
                match msg.cmd {
                    common::Command::Ping => {
                        TX_SIGNAL.signal(Command::Report);
                    }
                    _ => (),
                }
            } else {
                match msg.unwrap_err() {
                    NextRead(rg1) => rg = Some(rg1),
                    Codec(er) => {
                        warn!("malformed packet {:?}", er);
                    }
                }
            }
        }
    }
    Ok(())
}

static QUEUE: BBBuffer<QUEUE_LEN> = BBBuffer::new();

lock_free_static! {
    static mut RING_BUF : StaticRb<Report, RING_BUFLEN>  = StaticRb::default();
}

struct BufReaders {
    /// lossless stream of bytes, for compact transfer
    queue: bbqueue::Consumer<'static, QUEUE_LEN>,
    /// for real time but non-lossless status report
    /// this is a window representing [a few seconds ago] --to---> [present]
    /// each time a reporting is triggered, the entire buffer is sent and cleared.
    ringbuf: Caching<&'static SharedRb<Owning<[MaybeUninit<Report>; RING_BUFLEN]>>, false, true>,
}

struct Sending {
    queue: bbqueue::Producer<'static, QUEUE_LEN>,
    ringbuf: Caching<&'static SharedRb<Owning<[MaybeUninit<Report>; RING_BUFLEN]>>, true, false>,
}

static TX_SIGNAL: Signal<CriticalSectionRawMutex, Command> = Signal::new();

async fn report<'d, T: 'd + embassy_stm32::usb::Instance>(
    class: &mut cdc_acm::Sender<'d, Driver<'d, T>>,
    BufReaders { queue, ringbuf: rb }: &mut BufReaders,
) -> Result<(), Disconnected> {
    info!("reporting enabled");
    unsafe {
        let mut usb = unwrap!(USB_SIGNAL.subscriber());
        loop {
            if USB_STATE == UsbState::Connected
                || usb.next_message_pure().await == UsbState::Connected
            {
                break;
            }
        }
    }
    loop {
        let cmd = TX_SIGNAL.wait().await;
        // let rd = queue.read().unwrap();
        let reply = WireMessage {
            reports: rb.pop_iter().collect(),
            cmd,
            compact: Default::default(),
        };
        let rx: Result<heapless::Vec<u8, 1024>, postcard::Error> = postcard::to_vec_cobs(&reply);
        if let Ok(coded) = rx {
            let mut chunks = coded.into_iter().array_chunks::<64>();
            while let Some(p) = chunks.next() {
                class.write_packet(&p).await?;
            }
            if let Some(p) = chunks.into_remainder() {
                class.write_packet(p.as_slice()).await?;
            }
        } else {
            error!("{:?}", rx.unwrap_err())
        }
    }

    Ok(())
}
