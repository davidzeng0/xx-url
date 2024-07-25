use std::cell::Cell;
use std::io::{Cursor, IoSlice, Write as _};

use num_traits::FromPrimitive;
use xx_core::async_std::AsyncIterator;
use xx_core::coroutines::ops::AsyncFnOnce;
use xx_core::debug;
use xx_core::io::*;
use xx_core::os::epoll::PollFlag;
use xx_core::os::socket::Shutdown;
use xx_core::pointer::*;

use super::wire::{FramePacket, MutableFramePacket};
use super::*;

#[derive(Clone, Copy)]
pub struct FrameHeader {
	fin: bool,
	op: Op,
	mask: Option<u32>,
	len: u64
}

#[asynchronous]
async fn decode_length(len: u8, reader: &mut impl BufRead) -> Result<u64> {
	Ok(match len {
		len if len < 0x7e => len as u64,
		0x7e => reader.read_u16_be().await? as u64,
		_ => reader.read_u64_be().await?
	})
}

#[allow(clippy::checked_conversions, clippy::cast_possible_truncation)]
fn encode_len(len: u64, buf: &mut [u8; size_of::<u64>()]) -> (u8, &mut [u8]) {
	if len < 0x7e {
		(len as u8, &mut [])
	} else if len <= u16::MAX as u64 {
		let len = len as u16;

		read_into_slice(buf, &len.to_be_bytes());

		(0x7e, &mut buf[0..size_of::<u16>()])
	} else {
		buf.copy_from_slice(&len.to_be_bytes());

		(0x7f, &mut buf[..])
	}
}

#[asynchronous]
impl FrameHeader {
	async fn read(reader: &mut impl BufRead) -> Result<Option<Self>> {
		let header: [u8; FramePacket::minimum_packet_size()] = match reader.try_read_type().await? {
			Some(flags) => flags,
			None => return Ok(None)
		};

		let wire = FramePacket::new(&header).unwrap();
		let len = decode_length(wire.get_len(), reader).await?;
		let mask = if wire.get_masked() != 0 {
			Some(reader.read_u32_be().await?)
		} else {
			None
		};

		Ok(Some(Self {
			fin: wire.get_fin() != 0,
			op: Op::from_u8(wire.get_op()).unwrap_or_default(),
			mask,
			len
		}))
	}

	#[allow(clippy::missing_panics_doc)]
	fn write(&self, writer: &mut Cursor<&mut [u8]>) -> Result<()> {
		let mut length = [0u8; size_of::<u64>()];
		let (len, length) = encode_len(self.len, &mut length);

		let mut buf = [0u8; MutableFramePacket::minimum_packet_size()];
		let mut header = MutableFramePacket::new(&mut buf).unwrap();

		header.set_fin(self.fin as u8);
		header.set_resv(0);
		header.set_op(self.op as u8);
		header.set_masked(self.mask.is_some() as u8);
		header.set_len(len);

		writer.write_all(&buf)?;
		writer.write_all(length)?;

		if let Some(mask) = self.mask {
			writer.write_all(&mask.to_be_bytes())?;
		}

		Ok(())
	}
}

pub struct Shared {
	/* request options */
	pub max_message_length: usize,
	pub close_timeout: Duration,

	pub is_client: bool,
	pub close_state: Cell<Option<Shutdown>>
}

#[asynchronous]
impl Shared {
	pub const fn new(options: &WebSocketOptions, is_client: bool) -> Self {
		Self {
			max_message_length: options.max_message_length,
			close_timeout: options.close_timeout,

			is_client,
			close_state: Cell::new(None)
		}
	}

	pub fn can_read(&self) -> bool {
		!self
			.close_state
			.get()
			.is_some_and(|state| state != Shutdown::Write)
	}

	pub fn can_write(&self) -> bool {
		!self
			.close_state
			.get()
			.is_some_and(|state| state != Shutdown::Read)
	}

	fn should_close(&self) -> bool {
		self.close_state.get() == Some(Shutdown::Both)
	}

	fn shutdown(&self, how: Shutdown) -> bool {
		match self.close_state.get() {
			Some(cur) if cur == how => (),
			None => self.close_state.set(Some(how)),
			Some(_) => {
				self.close_state.set(Some(Shutdown::Both));

				return true;
			}
		}

		false
	}
}

#[asynchronous]
async fn do_close<S, F>(stream: &mut S, write_data: F) -> Result<()>
where
	S: ConnExtra,
	F: AsyncFnOnce(&mut S) -> Result<()>
{
	write_data.call_once(stream).await?;
	stream.shutdown(Shutdown::Write).await?;
	stream.poll(PollFlag::RdHangUp.into()).await?;

	Ok(())
}

#[asynchronous]
async fn close<T, S, F>(
	target: Ptr<T>, shared: &Shared, stream: &mut S, write_data: F
) -> Result<()>
where
	S: ConnExtra,
	F: AsyncFnOnce(&mut S) -> Result<()>
{
	let result = do_close(stream, write_data)
		.timeout(shared.close_timeout)
		.await;
	match result {
		Some(Ok(())) => (),
		Some(Err(err)) => debug!(target: target, "== Close was not clean: {:?}", err),
		None => debug!(target: target, "== Close timed out")
	}

	Ok(())
}

#[asynchronous]
async fn read_frame_data(
	stream: &mut impl BufRead, header: &mut FrameHeader, buf: &mut [u8]
) -> Result<usize> {
	read_into!(buf, header.len.try_into().unwrap_or(usize::MAX));

	let read = stream.read_fully(buf).await?;

	#[allow(clippy::arithmetic_side_effects)]
	(header.len -= read as u64);

	Ok(read)
}

pub struct Reader<'a, R> {
	pub(super) stream: R,
	pub(super) expect_continuation: &'a mut bool,
	pub(super) current_message: &'a mut Option<(Op, Vec<u8>)>,
	pub(super) data: &'a Shared
}

#[asynchronous]
impl<'a, R: BufRead + ConnExtra> Reader<'a, R> {
	pub async fn read_frame_header(&mut self) -> Result<Option<FrameHeader>> {
		if !self.data.can_read() {
			return Err(ErrorKind::Shutdown.into());
		}

		let Some(frame) = FrameHeader::read(&mut self.stream).await? else {
			self.data.shutdown(Shutdown::Read);

			return Ok(None);
		};

		if frame.op == Op::Invalid {
			return Err(WebSocketError::InvalidOpcode.into());
		}

		if frame.op.is_control() {
			if !frame.fin {
				return Err(
					WebSocketError::InvalidControlFrame("Fin not set on control frame").into()
				);
			}

			if frame.len > 0x7d {
				return Err(WebSocketError::InvalidControlFrame("Control frame too long").into());
			}
		} else {
			if *self.expect_continuation != (frame.op == Op::Continuation) {
				return Err(if *self.expect_continuation {
					WebSocketError::ExpectedContinuation
				} else {
					WebSocketError::UnexpectedContinuation
				}
				.into());
			}

			*self.expect_continuation = !frame.fin;
		}

		if frame.mask.is_some() && self.data.is_client {
			return Err(WebSocketError::ServerMasked.into());
		}

		if frame.op == Op::Close {
			self.data.shutdown(Shutdown::Read);
		}

		Ok(Some(frame))
	}

	pub async fn discard_frame_data(&mut self, header: &mut FrameHeader) -> Result<()> {
		loop {
			let available = self.stream.buffer().len();

			if let Some(remaining) = header.len.checked_sub(available as u64) {
				header.len = remaining;

				self.stream.discard();

				if self.stream.fill().await? == 0 {
					return Err(ErrorKind::UnexpectedEof.into());
				}
			} else {
				#[allow(clippy::cast_possible_truncation)]
				self.stream.consume(header.len as usize);

				header.len = 0;

				break;
			}
		}

		Ok(())
	}

	pub async fn read_frame_data(
		&mut self, header: &mut FrameHeader, buf: &mut [u8]
	) -> Result<usize> {
		read_frame_data(&mut self.stream, header, buf).await
	}

	#[must_use]
	pub const fn frames(self) -> Frames<'a, R> {
		Frames { reader: self }
	}
}

pub struct Frames<'a, R> {
	reader: Reader<'a, R>
}

#[asynchronous]
impl<'a, R: BufRead + ConnExtra> Frames<'a, R> {
	async fn read_frame(&mut self) -> Result<Option<Frame>> {
		let mut frame = match self.reader.read_frame_header().await? {
			Some(frame) => frame,
			None => {
				let close = Frame::Close(CloseCode::NoClose as u16, ControlFrame::new());

				return Ok(Some(close));
			}
		};

		if frame.len > self.reader.data.max_message_length as u64 {
			return Err(WebSocketError::MessageTooLong.into());
		}

		if frame.op.is_control() {
			let mut control = ControlFrame::new();

			#[allow(clippy::cast_possible_truncation)]
			(control.length = frame.len as u8);

			self.reader
				.read_frame_data(&mut frame, control.data_mut())
				.await?;

			if let Some(m) = &frame.mask {
				mask(control.data_mut(), *m);
			}

			Ok(Some(match frame.op {
				Op::Ping => Frame::Ping(control),
				Op::Pong => Frame::Pong(control),
				Op::Close => {
					let mut code = CloseCode::NoStatusCode as u16;

					if let Some(data) = control.data().get(0..2) {
						code = u16::from_be_bytes(data.try_into().unwrap());
						control.offset = 2;
					}

					if self.reader.data.should_close() {
						close(
							ptr!(&*self),
							self.reader.data,
							&mut self.reader.stream,
							|_: &mut R| async { Ok(()) }
						)
						.await?;
					}

					Frame::Close(code, control)
				}

				_ => unreachable!()
			}))
		} else {
			let (stream, current_message) =
				(&mut self.reader.stream, &mut *self.reader.current_message);

			let (_, buf) = current_message.get_or_insert_with(|| (frame.op, Vec::new()));

			self.reader
				.data
				.max_message_length
				.checked_sub(buf.len())
				.and_then(|remaining| (remaining as u64).checked_sub(frame.len))
				.ok_or(WebSocketError::MessageTooLong)?;

			let start = buf.len();

			#[allow(clippy::cast_possible_truncation)]
			let end = start.checked_add(frame.len as usize).unwrap();

			buf.resize(end, 0);

			let data = &mut buf[start..];

			read_frame_data(stream, &mut frame, data).await?;

			if let Some(m) = &frame.mask {
				mask(data, *m);
			}

			Ok(if frame.fin {
				let (op, buf) = current_message.take().unwrap();

				Some(match op {
					Op::Binary => Frame::Binary(buf),
					Op::Text => Frame::Text(String::from_utf8(buf)?),
					_ => unreachable!()
				})
			} else {
				None
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

#[asynchronous]
impl<R: BufRead + ConnExtra> AsyncIterator for Frames<'_, R> {
	type Item = Result<Frame>;

	async fn next(&mut self) -> Option<Self::Item> {
		if self.reader.data.can_read() {
			Some(self.next_frame().await)
		} else {
			None
		}
	}
}

pub struct Writer<'a, W> {
	pub(super) stream: W,
	pub(super) last_sent_message_op: &'a mut Option<Op>,
	pub(super) data: &'a Shared
}

#[asynchronous]
impl<'a, W: Write + ConnExtra> Writer<'a, W> {
	#[allow(clippy::impl_trait_in_params)]
	pub async fn send_frame<'b>(&mut self, frame: impl Into<BorrowedFrame<'b>>) -> Result<()> {
		if !self.data.can_write() {
			return Err(ErrorKind::Shutdown.into());
		}

		let frame = frame.into();

		let mut header = FrameHeader {
			fin: frame.fin,
			op: frame.op,
			mask: None,
			len: frame.payload.len() as u64
		};

		if self.data.is_client {
			header.mask = Some(0);
		}

		if header.op.is_control() {
			let additional = if header.op == Op::Close { 2 } else { 0 };

			#[allow(clippy::arithmetic_side_effects)]
			if header.len > 0x7d - additional {
				return Err(WebSocketError::UserInvalidControlFrame.into());
			}
		} else {
			if let Some(op) = *self.last_sent_message_op {
				if op != header.op {
					return Err(WebSocketError::DataTypeMismatch.into());
				}

				header.op = Op::Continuation;
			} else {
				*self.last_sent_message_op = Some(header.op);
			}

			if header.fin {
				*self.last_sent_message_op = None;
			}
		}

		let mut bytes = [0u8; 16];
		let header = {
			let mut writer = Cursor::new(&mut bytes[..]);

			header.write(&mut writer).unwrap();

			if header.op == Op::Close {
				writer.write_all(&frame.close_code.to_be_bytes()).unwrap();
			}

			#[allow(clippy::cast_possible_truncation)]
			let len = writer.position() as usize;

			&bytes[0..len]
		};

		let data = &mut [IoSlice::new(header), IoSlice::new(frame.payload)];

		if frame.op == Op::Close && self.data.shutdown(Shutdown::Write) {
			return close(
				ptr!(&*self),
				self.data,
				&mut self.stream,
				|stream: &mut W| async move {
					stream.write_all_vectored(data).await?;
					stream.flush().await
				}
			)
			.await;
		}

		let wrote = self.stream.write_all_vectored(data).await?;

		#[allow(clippy::arithmetic_side_effects)]
		if wrote < header.len() + frame.payload.len() {
			return Err(ErrorKind::UnexpectedEof.into());
		}

		if frame.op == Op::Close {
			self.stream.flush().await?;
		}

		Ok(())
	}
}
