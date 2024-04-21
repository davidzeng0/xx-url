use std::{
	cell::Cell,
	io::{Cursor, IoSlice, Write},
	net::{SocketAddr, ToSocketAddrs}
};

use num_traits::FromPrimitive;
use xx_core::{
	async_std::AsyncIterator,
	debug,
	os::{epoll::PollFlag, socket::Shutdown}
};

use super::{
	wire::{FrameHeaderPacket, MutableFrameHeaderPacket},
	*
};

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
		0x7e => reader.read_u16_be_or_err().await? as u64,
		_ => reader.read_u64_be_or_err().await?
	})
}

#[allow(clippy::checked_conversions, clippy::cast_possible_truncation)]
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

#[asynchronous]
impl FrameHeader {
	async fn read(reader: &mut impl BufRead) -> Result<Option<Self>> {
		let flags: [u8; 2] = match reader.read_type().await? {
			Some(flags) => flags,
			None => return Ok(None)
		};

		let wire = FrameHeaderPacket::new(&flags).unwrap();
		let len = decode_length(wire.get_len(), reader).await?;
		let mask = if wire.get_masked() != 0 {
			Some(reader.read_u32_be_or_err().await?)
		} else {
			None
		};

		Ok(Some(Self {
			fin: wire.get_fin() != 0,
			op: Op::from_u8(wire.get_op()).unwrap_or(Op::Invalid),
			mask,
			len
		}))
	}

	fn write(&self, writer: &mut Cursor<&mut [u8]>) -> Result<()> {
		let pos = writer.position().try_into().unwrap();

		#[allow(clippy::arithmetic_side_effects)]
		writer.set_position((pos + MutableFrameHeaderPacket::minimum_packet_size()) as u64);

		let len = encode_len(self.len, writer)?;
		let mut flags = MutableFrameHeaderPacket::new(&mut writer.get_mut()[pos..]).unwrap();

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

#[asynchronous]
impl<'a> Reader<'a> {
	pub async fn read_frame_header(&mut self) -> Result<Option<FrameHeader>> {
		if !self.web_socket.can_read() {
			return Err(Core::Shutdown.into());
		}

		let frame = match FrameHeader::read(&mut self.web_socket.stream).await? {
			Some(frame) => frame,
			None => {
				self.web_socket.shutdown(Shutdown::Read);

				return Ok(None);
			}
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
			if self.web_socket.expect_continuation != (frame.op == Op::Continuation) {
				return Err(if self.web_socket.expect_continuation {
					WebSocketError::ExpectedContinuation
				} else {
					WebSocketError::UnexpectedContinuation
				}
				.into());
			}

			self.web_socket.expect_continuation = !frame.fin;
		}

		if frame.mask.is_some() && self.web_socket.is_client {
			return Err(WebSocketError::ServerMasked.into());
		}

		if frame.op == Op::Close {
			self.web_socket.shutdown(Shutdown::Read);
		}

		Ok(Some(frame))
	}

	pub async fn discard_frame_data(&mut self, header: &mut FrameHeader) -> Result<()> {
		loop {
			let available = self.web_socket.stream.buffer().len();

			if let Some(remaining) = header.len.checked_sub(available as u64) {
				header.len = remaining;

				self.web_socket.stream.discard();

				if self.web_socket.stream.fill().await? == 0 {
					return Err(Core::UnexpectedEof.into());
				}
			} else {
				#[allow(clippy::cast_possible_truncation)]
				self.web_socket.stream.consume(header.len as usize);

				header.len = 0;

				break;
			}
		}

		Ok(())
	}

	async fn stream_read_frame_data(
		stream: &mut impl BufRead, header: &mut FrameHeader, buf: &mut [u8]
	) -> Result<usize> {
		read_into!(buf, header.len.try_into().unwrap_or(usize::MAX));

		let read = stream.read_exact(buf).await?;

		#[allow(clippy::arithmetic_side_effects)]
		(header.len -= read as u64);

		Ok(read)
	}

	pub async fn read_frame_data(
		&mut self, header: &mut FrameHeader, buf: &mut [u8]
	) -> Result<usize> {
		Self::stream_read_frame_data(&mut self.web_socket.stream, header, buf).await
	}

	#[must_use]
	pub const fn frames(self) -> Frames<'a> {
		Frames { reader: self }
	}
}

pub struct Frames<'a> {
	reader: Reader<'a>
}

#[asynchronous]
impl<'a> Frames<'a> {
	async fn read_frame(&mut self) -> Result<Option<Frame>> {
		let mut frame = match self.reader.read_frame_header().await? {
			Some(frame) => frame,
			None => {
				let close = Frame::Close(CloseCode::NoClose as u16, ControlFrame::new());

				return Ok(Some(close));
			}
		};

		if frame.len > self.reader.web_socket.max_message_length {
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

					self.reader.web_socket.maybe_close().await?;

					Frame::Close(code, control)
				}

				_ => unreachable!()
			}))
		} else {
			let (stream, current_message) = (
				&mut self.reader.web_socket.stream,
				&mut self.reader.web_socket.current_message
			);

			let (_, buf) = current_message.get_or_insert_with(|| (frame.op, Vec::new()));

			self.reader
				.web_socket
				.max_message_length
				.checked_sub(buf.len() as u64)
				.and_then(|remaining| remaining.checked_sub(frame.len))
				.ok_or(WebSocketError::MessageTooLong)?;

			let start = buf.len();
			let end = start.checked_add(frame.len.try_into().unwrap()).unwrap();

			buf.resize(end, 0);

			let data = &mut buf[start..];

			Reader::stream_read_frame_data(stream, &mut frame, data).await?;

			if let Some(m) = &frame.mask {
				mask(data, *m);
			}

			Ok(if frame.fin {
				let (op, buf) = current_message.take().unwrap();

				Some(match op {
					Op::Binary => Frame::Binary(buf),
					Op::Text => Frame::Text(String::from_utf8(buf).map_err(|_| Core::InvalidUtf8)?),
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
impl AsyncIterator for Frames<'_> {
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

#[asynchronous]
impl<'a> Writer<'a> {
	#[allow(clippy::impl_trait_in_params)]
	pub async fn send_frame<'b>(&mut self, frame: impl Into<BorrowedFrame<'b>>) -> Result<()> {
		if !self.web_socket.can_write() {
			return Err(Core::Shutdown.into());
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
			let additional = if header.op == Op::Close { 2 } else { 0 };

			#[allow(clippy::arithmetic_side_effects)]
			if header.len > 0x7d - additional {
				return Err(WebSocketError::UserInvalidControlFrame.into());
			}
		} else {
			if let Some(op) = self.web_socket.last_sent_message_op {
				if op != header.op {
					return Err(WebSocketError::DataTypeMismatch.into());
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
		let wrote = self
			.web_socket
			.stream
			.inner()
			.write_all_vectored(data)
			.await?;

		#[allow(clippy::arithmetic_side_effects)]
		if wrote < header.len() + frame.payload.len() {
			return Err(Core::UnexpectedEof.into());
		}

		if frame.op == Op::Close {
			self.web_socket.shutdown(Shutdown::Write);
			self.web_socket.maybe_close().await?;
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
	current_message: Option<(Op, Vec<u8>)>,

	is_client: bool,
	expect_continuation: bool,
	close_state: Cell<Option<Shutdown>>
}

#[asynchronous]
impl WebSocket {
	fn from(stream: BufReader<HttpStream>, options: &WebSocketOptions, is_client: bool) -> Self {
		Self {
			stream,

			max_message_length: options.max_message_length,
			close_timeout: options.close_timeout,

			last_sent_message_op: None,
			current_message: None,

			is_client,
			expect_continuation: false,
			close_state: Cell::new(None)
		}
	}

	pub async fn new(request: &mut WsRequest) -> Result<Self> {
		let stream = connect(request).await?;

		Ok(Self::from(stream, &request.options, true))
	}

	#[must_use]
	pub fn server(stream: BufReader<HttpStream>, options: &WebSocketOptions) -> Self {
		Self::from(stream, options, false)
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
			.get()
			.is_some_and(|state| state != Shutdown::Write)
	}

	pub fn can_write(&self) -> bool {
		!self
			.close_state
			.get()
			.is_some_and(|state| state != Shutdown::Read)
	}

	fn shutdown(&mut self, how: Shutdown) {
		/* this is the only shared value when split. prevent caching */
		let close_state = self.close_state.get();

		match close_state {
			None => self.close_state.set(Some(how)),
			Some(cur) if cur == how => (),
			Some(_) => self.close_state.set(Some(Shutdown::Both))
		}
	}

	async fn maybe_close(&mut self) -> Result<()> {
		if self.close_state.get() == Some(Shutdown::Both) {
			self.stream.inner().shutdown(Shutdown::Write).await?;

			if self
				.stream
				.inner()
				.poll(PollFlag::RdHangUp.into())
				.timeout(self.close_timeout)
				.await
				.is_none()
			{
				debug!(target: &*self, "== Close was not clean");
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
}

pub struct WebSocketServer {
	listener: TcpListener,
	options: WebSocketOptions
}

pub struct WebSocketHandle {
	stream: HttpStream,
	options: WebSocketOptions
}

#[asynchronous]
impl WebSocketHandle {
	async fn accept_websocket(self) -> Result<WebSocket> {
		struct WSServer {}

		let server = WSServer {};
		let stream = handle_upgrade(self.stream, &server)
			.timeout(self.options.handshake_timeout)
			.await
			.ok_or(WebSocketError::HandshakeTimeout)??;

		Ok(WebSocket::server(stream, &self.options))
	}
}

#[asynchronous(task)]
impl Task for WebSocketHandle {
	type Output<'a> = Result<WebSocket>;

	async fn run(self) -> Result<WebSocket> {
		self.accept_websocket().await
	}
}

#[asynchronous]
impl WebSocketServer {
	pub async fn bind<A>(addrs: A, options: WebSocketOptions) -> Result<Self>
	where
		A: ToSocketAddrs
	{
		let listener = Tcp::bind(addrs).await?;

		Ok(Self { listener, options })
	}

	pub async fn accept(&self) -> Result<WebSocketHandle> {
		let (socket, _) = self.listener.accept().await?;
		let stream = HttpStream::new(socket);

		Ok(WebSocketHandle { stream, options: self.options })
	}

	pub async fn local_addr(&self) -> Result<SocketAddr> {
		self.listener.local_addr().await
	}
}
