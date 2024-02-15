use std::{fmt, str::from_utf8, time::Duration};

use ::http::{Method, StatusCode};
use num_derive::FromPrimitive;
use xx_core::{
	async_std::io::{
		typed::{BufReadTyped, WriteTyped},
		*
	},
	coroutines::*,
	macros::*,
	pointer::*
};
use xx_pulse::impls::TaskExtensionsExt;

use super::*;
use crate::http::{stream::HttpStream, transfer::Request, HttpError, Version};

mod web_socket;
pub use web_socket::*;
mod request;
pub use request::{open, *};
mod errors;
pub use errors::*;

mod consts;
use consts::*;
mod handshake;
use handshake::*;
mod transfer;
use transfer::*;
mod wire;
use wire::Op;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromPrimitive)]
pub enum CloseCode {
	Normal              = 1000,
	GoingAway           = 1001,
	ProtocolError       = 1002,
	UnsupportedDataKind = 1003,
	Reserved            = 1004,
	NoStatusCode        = 1005,
	NoClose             = 1006,
	InvalidMessageData  = 1007,
	PolicyViolation     = 1008,
	MessageTooLong      = 1009,
	ExtensionsExpected  = 1010,
	InternalServerError = 1011,
	TlsHandshakeFailure = 1015
}

pub struct BorrowedFrame<'a> {
	op: Op,
	close_code: u16,
	payload: &'a [u8],
	fin: bool
}

impl<'a> Frame {
	pub fn text(payload: &'a str) -> BorrowedFrame<'a> {
		BorrowedFrame {
			op: Op::Text,
			close_code: 0,
			payload: payload.as_bytes(),
			fin: true
		}
	}

	pub fn binary(payload: &'a [u8]) -> BorrowedFrame<'a> {
		BorrowedFrame { op: Op::Binary, close_code: 0, payload, fin: true }
	}

	pub fn text_partial(payload: &'a str) -> BorrowedFrame<'a> {
		BorrowedFrame {
			op: Op::Text,
			close_code: 0,
			payload: payload.as_bytes(),
			fin: false
		}
	}

	pub fn binary_partial(payload: &'a [u8]) -> BorrowedFrame<'a> {
		BorrowedFrame { op: Op::Binary, close_code: 0, payload, fin: false }
	}

	pub fn ping(payload: &'a [u8]) -> BorrowedFrame<'a> {
		BorrowedFrame { op: Op::Ping, close_code: 0, payload, fin: true }
	}

	pub fn pong(payload: &'a [u8]) -> BorrowedFrame<'a> {
		BorrowedFrame { op: Op::Pong, close_code: 0, payload, fin: true }
	}

	pub fn close(code: u16, payload: &'a [u8]) -> BorrowedFrame<'a> {
		BorrowedFrame {
			op: Op::Close,
			close_code: code,
			payload,
			fin: true
		}
	}
}

impl<'a> From<&'a Frame> for BorrowedFrame<'a> {
	fn from(frame: &'a Frame) -> Self {
		match frame {
			Frame::Ping(frame) => Frame::ping(frame.as_ref()),
			Frame::Pong(frame) => Frame::pong(frame.as_ref()),
			Frame::Close(code, payload) => Frame::close(*code, payload.as_ref()),
			Frame::Binary(payload) => Frame::binary(payload.as_ref()),
			Frame::Text(payload) => Frame::text(payload.as_ref())
		}
	}
}

impl<'a> From<&'a str> for BorrowedFrame<'a> {
	fn from(value: &'a str) -> Self {
		Frame::text(value)
	}
}

impl<'a> From<&'a [u8]> for BorrowedFrame<'a> {
	fn from(value: &'a [u8]) -> Self {
		Frame::binary(value)
	}
}

pub struct ControlFrame {
	data: [u8; Self::MAX_LENGTH],
	offset: u8,
	length: u8
}

impl ControlFrame {
	pub const MAX_LENGTH: usize = 0x7d;

	pub fn new() -> Self {
		Self { data: [0; Self::MAX_LENGTH], offset: 0, length: 0 }
	}

	pub fn data(&self) -> &[u8] {
		&self.data[self.offset as usize..self.length as usize]
	}

	pub fn data_mut(&mut self) -> &mut [u8] {
		&mut self.data[self.offset as usize..self.length as usize]
	}
}

impl fmt::Debug for ControlFrame {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt.debug_tuple("ControlFrame").field(&self.data()).finish()
	}
}

impl AsRef<[u8]> for ControlFrame {
	fn as_ref(&self) -> &[u8] {
		self.data()
	}
}

#[derive(Debug)]
pub enum Frame {
	Ping(ControlFrame),
	Pong(ControlFrame),
	Close(u16, ControlFrame),
	Text(String),
	Binary(Vec<u8>)
}

impl fmt::Display for Frame {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Frame::Ping(frame) => fmt.debug_tuple("Ping").field(&frame.data()).finish(),
			Frame::Pong(frame) => fmt.debug_tuple("Pong").field(&frame.data()).finish(),
			Frame::Close(code, frame) => {
				let mut close = fmt.debug_struct("Close");

				close.field("code", code);

				match from_utf8(frame.data()) {
					Ok(msg) => close.field("message", &msg),
					Err(_) => close.field("message", &frame.data())
				};

				close.finish()
			}

			Frame::Text(data) => fmt.debug_tuple("Text").field(&data).finish(),
			Frame::Binary(data) => fmt.debug_tuple("Text").field(&data).finish()
		}
	}
}

pub fn mask(data: &mut [u8], mut mask: u32) {
	let (pre, align, post) = unsafe { data.align_to_mut::<u32>() };

	for byte in pre.iter_mut() {
		*byte ^= (mask >> 24) as u8;
		mask = mask.rotate_left(8);
	}

	/* this loop gets vectorized */
	for val in align.iter_mut() {
		*val ^= mask.to_be();
	}

	for byte in post.iter_mut() {
		*byte ^= (mask >> 24) as u8;
		mask = mask.rotate_left(8);
	}
}
