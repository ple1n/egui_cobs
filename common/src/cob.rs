use core::{
    iter::Peekable,
    marker::PhantomData,
    ops::{Range, RangeFrom},
};

use defmt::Format;
use serde::{de::DeserializeOwned, Deserialize};

use crate::*;

pub struct COBSeek<'s> {
    slice: &'s [u8],
    pub skip: usize,
    /// describes the first byte after skipped ones
    pub state: COBState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum COBState {
    Started,
    Data,
    Sentinel,
}

impl<'s> COBSeek<'s> {
    const SENTINEL: u8 = 0;
    pub fn new(sl: &'s [u8]) -> Self {
        Self {
            slice: sl,
            skip: 0,
            state: COBState::Started,
        }
    }
}

pub struct COB<'s, T>(
    COBSeek<'s>,
    PhantomData<T>,
    &'s mut [u8],
    Option<Range<usize>>,
);

impl<'s, T> COB<'s, T> {
    /// must pass identical copy
    pub fn new(buf: &'s mut [u8], copy: &'s [u8]) -> Self {
        Self(COBSeek::new(&copy), PhantomData, buf, None)
    }
}

#[derive(Format, Debug)]
pub enum COBErr {
    Codec(postcard::Error),
    NextRead(RangeFrom<usize>),
}

impl From<postcard::Error> for COBErr {
    fn from(value: postcard::Error) -> Self {
        COBErr::Codec(value)
    }
}

impl<'s, T: DeserializeOwned> Iterator for COB<'s, T> {
    type Item = Result<T, COBErr>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut now: Option<Range<usize>> = self.3.take();
        let next: &mut Option<Range<usize>> = &mut self.3;
        for (range, isdata) in &mut self.0 {
            if isdata {
                if now.is_none() {
                    now = Some(range);
                } else if next.is_none() {
                    *next = Some(range);
                    break;
                }
            }
        }

        if let Some(range) = now {
            let de: Result<T, postcard::Error> =
                postcard::from_bytes_cobs(&mut self.2[range.clone()]);
            if next.is_none() {
                if de.is_err() {
                    self.2[..range.len()].copy_from_slice(&self.0.slice[range.clone()]);
                    self.2[range.len()..].fill(0);
                    Some(Err(COBErr::NextRead(range.len() + 1..)))
                } else {
                    self.2.fill(0);
                    Some(de.map_err(|k| k.into()))
                }
            } else {
                Some(de.map_err(|k| k.into()))
            }
        } else {
            None
        }
    }
}

impl<'s> Iterator for COBSeek<'s> {
    /// true if its data
    type Item = (Range<usize>, bool);
    fn next(&mut self) -> Option<Self::Item> {
        if self.state == COBState::Started {
            self.state = if self.slice[0] == Self::SENTINEL {
                COBState::Sentinel
            } else {
                COBState::Data
            };
        }
        if self.skip == self.slice.len() {
            None
        } else {
            let start = self.skip;
            let is_data;
            let range = if self.state == COBState::Data {
                is_data = true;
                if let Some(pos) = self.slice[start..]
                    .iter()
                    .position(|k| *k == Self::SENTINEL)
                {
                    self.state = COBState::Sentinel;
                    start..(start + pos)
                } else {
                    start..self.slice.len()
                }
            } else {
                is_data = false;
                if let Some(pos) = self.slice[start..]
                    .iter()
                    .position(|k| *k != Self::SENTINEL)
                {
                    self.state = COBState::Data;
                    start..(start + pos)
                } else {
                    start..self.slice.len()
                }
            };
            self.skip += range.len();
            Some((range, is_data))
        }
    }
}

