pub mod web_socket;
use std::{fmt, mem::MaybeUninit};

pub use web_socket::*;
pub mod request;
pub use request::*;

use self::wire::Op;

mod consts;
mod handshake;
mod transfer;
mod wire;

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

pub struct ControlFrame {
	data: [u8; 0x7d],
	offset: u8,
	length: u8
}

impl ControlFrame {
	fn new() -> Self {
		Self {
			#[allow(invalid_value)]
			data: unsafe { MaybeUninit::uninit().assume_init() },
			offset: 0,
			length: 0
		}
	}

	pub fn data(&self) -> &[u8] {
		&self.data[self.offset as usize..self.length as usize]
	}
}

impl AsRef<[u8]> for ControlFrame {
	fn as_ref(&self) -> &[u8] {
		self.data()
	}
}

impl fmt::Debug for ControlFrame {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.as_ref().fmt(fmt)
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
