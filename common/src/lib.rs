#![cfg_attr(not(test), no_std)]

use core::mem::size_of;

use bytemuck::NoUninit;
use defmt::Format;
use heapless::Vec;
use serde::{self, Deserialize, Serialize};
use static_assertions::const_assert;

#[derive(Serialize, Deserialize, Default, Debug, Clone, Format)]
pub struct WireMessage {
    pub ack: u64
}

pub const MAX_PACKET_SIZE: usize = 4096;

const_assert!(size_of::<WireMessage>() < MAX_PACKET_SIZE);

pub mod cob;
pub mod num;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Format)]
pub struct Report {

}
