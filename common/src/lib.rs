#![cfg_attr(not(test), no_std)]

use core::{default, mem::size_of};

use bytemuck::NoUninit;
use defmt::Format;
use heapless::Vec;
use serde::{self, Deserialize, Serialize};
use static_assertions::const_assert;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WireMessage {
    pub cmd: Command,
    pub reports: Vec<Report, 2>,
    pub compact: Vec<u8, 4>,
}

impl Default for WireMessage {
    fn default() -> Self {
        Self {
            cmd: Command::Ping,
            reports: Default::default(),
            compact: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Format)]
pub enum Command {
    #[default]
    Report,
    Ping,
}

pub const MAX_PACKET_SIZE: usize = 64;

const_assert!(size_of::<WireMessage>() < MAX_PACKET_SIZE);

pub mod cob;
pub mod num;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Format)]
pub struct Report {
    pub particle: Particles
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Format)]
// Valid range 0 to 700
pub struct Particles {
    pub pm1: u16,
    pub pm2: u16,
    pub pm10: u16,
}
