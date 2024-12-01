#![allow(unreachable_code)]
#![allow(async_fn_in_trait)]

use std::collections::VecDeque;

use bufrecv::Merge;
pub use derive_new::new;

pub use anyhow::Result;

pub mod bufrecv;
pub mod conn;

use common::{Particles, Report};
use tracing::info;

#[derive(Default, Clone)]
pub struct Data {
    pub points: VecDeque<Report>,
}

const MAX_POINTS: usize = 360;

impl Merge for Data {
    type Delta = Self;
    fn merge(&mut self, incoming: Self::Delta) {
        while self.points.len() > MAX_POINTS {
            self.points.pop_back();
        }
        for point in incoming.points {
            self.points.push_front(point);            
        }
    }
}
