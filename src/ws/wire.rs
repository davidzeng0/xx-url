use num_derive::FromPrimitive;
use pnet_macros::packet;
use pnet_macros_support::types::{u1, u3, u4, u7};

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, FromPrimitive)]
#[repr(i8)]
pub enum Op {
	#[default]
	Invalid      = -1,
	Continuation = 0x0,
	Text         = 0x1,
	Binary       = 0x2,
	Close        = 0x8,
	Ping         = 0x9,
	Pong         = 0xa
}

impl Op {
	#[must_use]
	pub const fn is_control(self) -> bool {
		matches!(self, Self::Ping | Self::Pong | Self::Close)
	}
}

#[packet]
#[allow(dead_code)]
pub struct Frame {
	pub fin: u1,
	pub resv: u3,
	pub op: u4,
	pub masked: u1,
	pub len: u7,

	#[payload]
	pub payload: Vec<u8>
}
