#![allow(unreachable_pub)]

use std::fmt;
use std::str::from_utf8;
use std::time::Duration;

use ::http::{Method, StatusCode};
use num_derive::FromPrimitive;
use xx_core::async_std::io::typed::{BufReadTyped, WriteTyped};
use xx_core::async_std::io::*;
use xx_core::coroutines::*;
use xx_core::macros::*;
use xx_pulse::impls::TaskExt;

use super::*;
use crate::http::stream::*;
use crate::http::transfer::Request;
use crate::http::{Headers, HttpError, Payload, TryIntoHeaderName, TryIntoHeaderValue, Version};

mod conn;
mod consts;
mod errors;
mod handshake;
mod request;
mod stream;
mod transfer;
mod wire;

pub use conn::*;
pub use errors::*;
pub use request::{open, *};
use wire::Op;

use self::consts::*;
use self::handshake::*;
use self::transfer::*;

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

impl From<CloseCode> for u16 {
	fn from(value: CloseCode) -> Self {
		value as Self
	}
}

pub struct BorrowedFrame<'payload> {
	op: Op,
	close_code: u16,
	payload: &'payload [u8],
	fin: bool
}

impl<'payload> Frame {
	#[must_use]
	pub const fn text(payload: &'payload str) -> BorrowedFrame<'payload> {
		BorrowedFrame {
			op: Op::Text,
			close_code: 0,
			payload: payload.as_bytes(),
			fin: true
		}
	}

	#[must_use]
	pub const fn binary(payload: &'payload [u8]) -> BorrowedFrame<'payload> {
		BorrowedFrame { op: Op::Binary, close_code: 0, payload, fin: true }
	}

	#[must_use]
	pub const fn text_partial(payload: &'payload str) -> BorrowedFrame<'payload> {
		BorrowedFrame {
			op: Op::Text,
			close_code: 0,
			payload: payload.as_bytes(),
			fin: false
		}
	}

	#[must_use]
	pub const fn binary_partial(payload: &'payload [u8]) -> BorrowedFrame<'payload> {
		BorrowedFrame { op: Op::Binary, close_code: 0, payload, fin: false }
	}

	#[must_use]
	pub const fn ping(payload: &'payload [u8]) -> BorrowedFrame<'payload> {
		BorrowedFrame { op: Op::Ping, close_code: 0, payload, fin: true }
	}

	#[must_use]
	pub const fn pong(payload: &'payload [u8]) -> BorrowedFrame<'payload> {
		BorrowedFrame { op: Op::Pong, close_code: 0, payload, fin: true }
	}

	#[must_use]
	#[allow(clippy::impl_trait_in_params)]
	pub fn close(code: impl Into<u16>, payload: &'payload [u8]) -> BorrowedFrame<'payload> {
		BorrowedFrame {
			op: Op::Close,
			close_code: code.into(),
			payload,
			fin: true
		}
	}
}

impl<'payload> From<&'payload Frame> for BorrowedFrame<'payload> {
	fn from(frame: &'payload Frame) -> Self {
		match frame {
			Frame::Ping(frame) => Frame::ping(frame.as_ref()),
			Frame::Pong(frame) => Frame::pong(frame.as_ref()),
			Frame::Close(code, payload) => Frame::close(*code, payload.as_ref()),
			Frame::Binary(payload) => Frame::binary(payload.as_ref()),
			Frame::Text(payload) => Frame::text(payload.as_ref())
		}
	}
}

impl<'payload> From<&'payload str> for BorrowedFrame<'payload> {
	fn from(value: &'payload str) -> Self {
		Frame::text(value)
	}
}

impl<'payload> From<&'payload [u8]> for BorrowedFrame<'payload> {
	fn from(value: &'payload [u8]) -> Self {
		Frame::binary(value)
	}
}

#[derive(Clone, Copy)]
pub struct ControlFrame {
	data: [u8; Self::MAX_LENGTH],
	offset: u8,
	length: u8
}

impl ControlFrame {
	pub const MAX_LENGTH: usize = 0x7d;

	#[must_use]
	pub const fn new() -> Self {
		Self { data: [0; Self::MAX_LENGTH], offset: 0, length: 0 }
	}

	#[must_use]
	pub fn data(&self) -> &[u8] {
		&self.data[self.offset as usize..self.length as usize]
	}

	pub fn data_mut(&mut self) -> &mut [u8] {
		&mut self.data[self.offset as usize..self.length as usize]
	}
}

impl Default for ControlFrame {
	fn default() -> Self {
		Self::new()
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
			Self::Ping(frame) => fmt.debug_tuple("Ping").field(&frame.data()).finish(),
			Self::Pong(frame) => fmt.debug_tuple("Pong").field(&frame.data()).finish(),
			Self::Close(code, frame) => {
				let mut close = fmt.debug_struct("Close");

				close.field("code", code);

				match from_utf8(frame.data()) {
					Ok(msg) => close.field("message", &msg),
					Err(_) => close.field("message", &frame.data())
				};

				close.finish()
			}

			Self::Text(data) => fmt.debug_tuple("Text").field(&data).finish(),
			Self::Binary(data) => fmt.debug_tuple("Text").field(&data).finish()
		}
	}
}

pub fn mask(data: &mut [u8], mut mask: u32) {
	/* Safety: transmute [u8; 4] to u32 is ok */
	#[allow(unsafe_code)]
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
