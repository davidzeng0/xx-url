use std::{
	io::{Cursor, IoSlice, Write},
	time::Duration
};

use num_traits::FromPrimitive;
use xx_core::{
	async_std::{io::*, AsyncIterator},
	debug,
	error::*,
	os::socket::Shutdown,
	pointer::MutPtr,
	read_into
};
use xx_pulse::*;

use super::{transfer::connect, wire::*, BorrowedFrame, ControlFrame, Frame, WsRequest};
use crate::http::stream::HttpStream;

pub struct FrameHeader {
	fin: bool,
	op: Op,
	mask: Option<u32>,
	len: u64
}

#[async_fn]
async fn decode_length(len: u8, reader: &mut TypedReader<impl BufRead>) -> Result<u64> {
	if len < 0x7e {
		Ok(len as u64)
	} else if len == 0x7e {
		Ok(reader.read_u16_be_or_err().await? as u64)
	} else {
		Ok(reader.read_u64_be_or_err().await?)
	}
}

fn encode_len(len: u64, writer: &mut Cursor<&mut [u8]>) -> Result<u8> {
	if len < 0x7e {
		Ok(len as u8)
	} else if len <= u16::MAX as u64 {
		let len = len as u16;

		writer.write_all(&len.to_be_bytes())?;

		Ok(0x7e)
	} else {
		writer.write_all(&len.to_be_bytes())?;

		Ok(0x7f)
	}
}

#[async_fn]
impl FrameHeader {
	async fn read(reader: &mut impl BufRead) -> Result<Self> {
		let mut reader = TypedReader::new(reader.as_ref());
		let flags: [u8; 2] = reader.read_type_or_err().await?;

		let wire = FrameHeaderPacket::new(&flags).unwrap();
		let len = decode_length(wire.get_len(), &mut reader).await?;
		let mask = if wire.get_masked() != 0 {
			Some(reader.read_u32_be_or_err().await?)
		} else {
			None
		};

		Ok(Self {
			fin: wire.get_fin() != 0,
			op: Op::from_u8(wire.get_op()).unwrap_or(Op::Invalid),
			mask,
			len
		})
	}

	fn write(&self, writer: &mut Cursor<&mut [u8]>) -> Result<()> {
		let pos = writer.position() as usize;

		writer.set_position((pos + MutableFrameHeaderPacket::minimum_packet_size()) as u64);

		let len = encode_len(self.len, writer)?;

		let mut flags = MutableFrameHeaderPacket::new(&mut writer.get_mut()[pos as usize..])
			.ok_or_else(|| Error::Simple(ErrorKind::InvalidInput))?;
		flags.set_fin(self.fin as u8);
		flags.set_resv(0);
		flags.set_op(self.op as u8);
		flags.set_masked(self.mask.is_some() as u8);
		flags.set_len(len);

		if let Some(mask) = self.mask {
			writer.write_all(&mask.to_be_bytes())?;
		}

		Ok(())
	}
}

pub struct Reader<'a> {
	web_socket: &'a mut WebSocket
}

#[async_fn]
impl<'a> Reader<'a> {
	pub async fn read_frame_header(&mut self) -> Result<FrameHeader> {
		if !self.web_socket.can_read() {
			return Err(Error::new(ErrorKind::Other, "Read end is shutdown"));
		}

		let frame = FrameHeader::read(&mut self.web_socket.stream).await?;

		if frame.op == Op::Invalid {
			return Err(Error::new(ErrorKind::InvalidData, "Invalid opcode"));
		}

		if frame.op.is_control() {
			if !frame.fin {
				return Err(Error::new(
					ErrorKind::InvalidData,
					"Fin not set on a control frame"
				));
			}

			if frame.len > 0x7d {
				return Err(Error::new(
					ErrorKind::InvalidData,
					"Control frame exceeded maximum size"
				));
			}
		} else {
			if self.web_socket.expect_continuation != (frame.op == Op::Continuation) {
				if self.web_socket.expect_continuation {
					return Err(Error::new(
						ErrorKind::InvalidData,
						"Expected a continuation frame"
					));
				} else {
					return Err(Error::new(
						ErrorKind::InvalidData,
						"Unexpected continuation frame"
					));
				}
			}

			self.web_socket.expect_continuation = !frame.fin;
		}

		if frame.mask.is_some() && self.web_socket.is_client {
			return Err(Error::new(
				ErrorKind::InvalidData,
				"Received masked frame from server"
			));
		}

		if frame.op == Op::Close {
			self.web_socket.shutdown(Shutdown::Read);
		}

		Ok(frame)
	}

	pub async fn discard_frame_data(&mut self, header: &mut FrameHeader) -> Result<()> {
		loop {
			let available = self.web_socket.stream.buffer().len();

			if header.len > available as u64 {
				header.len -= available as u64;

				self.web_socket.stream.discard();

				if self.web_socket.stream.fill().await? == 0 {
					return Err(Error::new(
						ErrorKind::UnexpectedEof,
						"End of file mid frame"
					));
				}
			} else {
				self.web_socket.stream.consume(header.len as usize);

				header.len = 0;

				break;
			}
		}

		Ok(())
	}

	pub async fn read_frame_data(
		&mut self, header: &mut FrameHeader, buf: &mut [u8]
	) -> Result<usize> {
		read_into!(buf, header.len as usize);

		match self.web_socket.stream.read_exact_or_err(buf).await {
			Ok(()) => {
				header.len -= buf.len() as u64;

				return Ok(buf.len());
			}

			Err(err) => Err(if err.kind() == ErrorKind::UnexpectedEof {
				Error::new(ErrorKind::UnexpectedEof, "End of file mid frame")
			} else {
				err
			})
		}
	}

	pub fn frames(self) -> Frames<'a> {
		Frames { reader: self, current_message: None }
	}
}

pub struct Frames<'a> {
	reader: Reader<'a>,
	current_message: Option<(Op, Vec<u8>)>
}

#[async_fn]
impl<'a> Frames<'a> {
	async fn read_frame(&mut self) -> Result<Option<Frame>> {
		let mut frame = self.reader.read_frame_header().await?;

		if frame.op.is_control() {
			let mut control = ControlFrame::new();

			control.length = self
				.reader
				.read_frame_data(&mut frame, &mut control.data)
				.await? as u8;
			Ok(Some(match frame.op {
				Op::Ping => Frame::Ping(control),
				Op::Pong => Frame::Pong(control),
				Op::Close => {
					let mut code = 1005;

					if control.data().len() >= 2 {
						code = u16::from_be_bytes([control.data()[0], control.data()[1]]);
						control.offset = 2;
					}

					Frame::Close(code, control)
				}

				_ => unreachable!()
			}))
		} else {
			let (_, buf) = self
				.current_message
				.get_or_insert_with(|| (frame.op, Vec::new()));

			let remaining = self
				.reader
				.web_socket
				.max_message_length
				.checked_sub(buf.len() as u64);
			let remaining = remaining.map(|len| len.checked_sub(frame.len)).flatten();

			remaining
				.ok_or_else(|| Error::new(ErrorKind::Other, "Maximum message length exceeded"))?;
			buf.reserve(frame.len as usize);

			unsafe {
				let start = buf.len();
				let end = start + frame.len as usize;

				self.reader
					.read_frame_data(&mut frame, buf.get_unchecked_mut(start..end))
					.await?;
				buf.set_len(end);
			}

			Ok(if !frame.fin {
				None
			} else {
				let (op, buf) = self.current_message.take().unwrap();

				Some(match op {
					Op::Binary => Frame::Binary(buf),
					Op::Text => {
						Frame::Text(String::from_utf8(buf).map_err(|_| invalid_utf8_error())?)
					}
					_ => unreachable!()
				})
			})
		}
	}

	async fn next_frame(&mut self) -> Result<Frame> {
		loop {
			if let Some(frame) = self.read_frame().await? {
				return Ok(frame);
			}
		}
	}
}

#[async_trait_impl]
impl<'a> AsyncIterator for Frames<'a> {
	type Item = Result<Frame>;

	async fn next(&mut self) -> Option<Self::Item> {
		if self.reader.web_socket.can_read() {
			Some(self.next_frame().await)
		} else {
			None
		}
	}
}

pub struct Writer<'a> {
	web_socket: &'a mut WebSocket
}

#[async_fn]
impl<'a> Writer<'a> {
	pub async fn send_frame<'b>(&mut self, frame: impl Into<BorrowedFrame<'b>>) -> Result<()> {
		if !self.web_socket.can_write() {
			return Err(Error::new(ErrorKind::Other, "Write end is shutdown"));
		}

		let frame = frame.into();
		let mut header = FrameHeader {
			fin: frame.fin,
			op: frame.op,
			mask: None,
			len: frame.payload.len() as u64
		};

		if self.web_socket.is_client {
			header.mask = Some(0);
		}

		if header.op.is_control() {
			if header.op == Op::Close {
				header.len += 2;
			}

			if header.len > 0x7d {
				return Err(Error::new(
					ErrorKind::InvalidInput,
					"Control frame data too long"
				));
			}
		} else {
			if let Some(op) = self.web_socket.last_sent_message_op {
				if op != header.op {
					return Err(Error::new(
						ErrorKind::InvalidInput,
						"Cannot send mismatching data types in chunks"
					));
				}

				header.op = Op::Continuation;
			} else {
				self.web_socket.last_sent_message_op = Some(header.op);
			}

			if header.fin {
				self.web_socket.last_sent_message_op = None;
			}
		}

		let mut bytes = [0u8; 16];
		let mut writer = Cursor::new(&mut bytes[..]);

		header.write(&mut writer).unwrap();

		if header.op == Op::Close {
			writer.write_all(&frame.close_code.to_be_bytes()).unwrap();

			self.web_socket.shutdown(Shutdown::Write);
		}

		let header_len = writer.position() as usize;

		self.web_socket
			.write_all(&mut [&bytes[0..header_len], frame.payload])
			.await?;

		if self.web_socket.close_state == Some(Shutdown::Both) {
			let mut byte = [0u8; 1];

			self.web_socket
				.stream
				.inner()
				.shutdown(Shutdown::Write)
				.await?;

			match select(
				sleep(self.web_socket.close_timeout),
				self.web_socket.stream.read(&mut byte)
			)
			.await
			{
				Select::First(..) => debug!(target: self, "== Close was not clean"),
				_ => ()
			}
		}

		Ok(())
	}
}

pub struct WebSocket {
	stream: BufReader<HttpStream>,

	/* request options */
	max_message_length: u64,
	close_timeout: Duration,

	last_sent_message_op: Option<Op>,
	is_client: bool,
	expect_continuation: bool,
	close_state: Option<Shutdown>
}

#[async_fn]
impl WebSocket {
	pub async fn new(request: &WsRequest) -> Result<Self> {
		let stream = connect(request).await?;

		Ok(Self {
			stream,

			max_message_length: request.options.max_message_length,
			close_timeout: request.options.close_timeout,

			last_sent_message_op: None,
			is_client: true,
			expect_continuation: false,
			close_state: None
		})
	}

	pub fn set_max_message_length(&mut self, max: u64) -> &mut Self {
		self.max_message_length = max;
		self
	}

	pub fn set_close_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.close_timeout = timeout;
		self
	}

	pub fn can_read(&self) -> bool {
		!self
			.close_state
			.is_some_and(|state| state != Shutdown::Write)
	}

	pub fn can_write(&self) -> bool {
		!self
			.close_state
			.is_some_and(|state| state != Shutdown::Read)
	}

	fn shutdown(&mut self, how: Shutdown) {
		match self.close_state {
			None => self.close_state = Some(how),
			Some(cur) if cur == how => (),
			Some(_) => self.close_state = Some(Shutdown::Both)
		}
	}

	async fn write_all(&mut self, mut bufs: &mut [&[u8]]) -> Result<()> {
		while bufs.len() > 0 {
			let mut wrote = {
				if bufs.len() == 2 {
					self.stream
						.inner()
						.write_vectored(&[IoSlice::new(bufs[0]), IoSlice::new(bufs[1])])
						.await?
				} else {
					self.stream
						.inner()
						.write_vectored(&[IoSlice::new(bufs[0])])
						.await?
				}
			};

			if wrote == 0 {
				return Err(Error::new(
					ErrorKind::UnexpectedEof,
					"End of file while writing frame"
				));
			}

			while bufs.len() > 0 {
				if wrote >= bufs[0].len() {
					wrote -= bufs[0].len();
					bufs = &mut bufs[1..];
				} else {
					bufs[0] = &bufs[0][wrote..];

					break;
				}
			}
		}

		Ok(())
	}

	pub fn reader(&mut self) -> Reader<'_> {
		Reader { web_socket: self }
	}

	pub fn writer(&mut self) -> Writer<'_> {
		Writer { web_socket: self }
	}

	/// Safety: same thread
	pub fn split(&mut self) -> (Reader<'_>, Writer<'_>) {
		let mut this = MutPtr::from(self);

		(
			Reader { web_socket: this.as_mut() },
			Writer { web_socket: this.as_mut() }
		)
	}
}
