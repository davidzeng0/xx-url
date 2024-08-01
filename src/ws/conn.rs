use std::net::{SocketAddr, ToSocketAddrs};

use xx_pulse::net::*;

use super::stream::Shared;
use super::*;

pub type WsReader<'a> = stream::Reader<'a, &'a mut BufReader<HttpConn>>;
pub type WsFrames<'a> = stream::Frames<'a, &'a mut BufReader<HttpConn>>;
pub type WsWriter<'a> = stream::Writer<'a, &'a mut HttpConn>;

pub type WsReadHalf<'a> = stream::Reader<'a, BufReadHalf<'a, HttpConnReadHalf<'a>>>;
pub type WsReadHalfFrames<'a> = stream::Frames<'a, BufReadHalf<'a, HttpConnReadHalf<'a>>>;
pub type WsWriteHalf<'a> = stream::Writer<'a, HttpConnWriteHalf<'a>>;

pub struct WebSocket {
	stream: BufReader<HttpConn>,

	last_sent_message_op: Option<Op>,
	current_message: Option<(Op, Vec<u8>)>,

	expect_continuation: bool,
	data: Shared
}

#[asynchronous]
impl WebSocket {
	const fn from(
		stream: BufReader<HttpConn>, options: &WebSocketOptions, is_client: bool
	) -> Self {
		Self {
			stream,

			last_sent_message_op: None,
			current_message: None,

			expect_continuation: false,
			data: Shared::new(options, is_client)
		}
	}

	pub async fn new(request: &mut WsRequest) -> Result<Self> {
		let stream = connect(request).await?;

		Ok(Self::from(stream, &request.options, true))
	}

	#[must_use]
	pub const fn server(stream: BufReader<HttpConn>, options: &WebSocketOptions) -> Self {
		Self::from(stream, options, false)
	}

	pub fn set_max_message_length(&mut self, max: usize) -> &mut Self {
		self.data.max_message_length = max;
		self
	}

	pub fn set_close_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.data.close_timeout = timeout;
		self
	}

	pub fn can_read(&self) -> bool {
		self.data.can_read()
	}

	pub fn can_write(&self) -> bool {
		self.data.can_write()
	}

	pub fn reader(&mut self) -> WsReader<'_> {
		WsReader {
			stream: &mut self.stream,
			expect_continuation: &mut self.expect_continuation,
			current_message: &mut self.current_message,
			data: &self.data
		}
	}

	pub fn writer(&mut self) -> WsWriter<'_> {
		WsWriter {
			stream: self.stream.inner_mut(),
			last_sent_message_op: &mut self.last_sent_message_op,
			data: &self.data
		}
	}

	#[must_use]
	pub fn frames(&mut self) -> WsFrames<'_> {
		self.reader().frames()
	}

	#[allow(clippy::impl_trait_in_params)]
	pub async fn send_frame<'b>(&mut self, frame: impl Into<BorrowedFrame<'b>>) -> Result<()> {
		self.writer().send_frame(frame).await
	}

	pub fn split(&mut self) -> (WsReadHalf<'_>, WsWriteHalf<'_>) {
		let (reader, writer) = self.stream.split();

		(
			WsReadHalf {
				stream: reader,
				expect_continuation: &mut self.expect_continuation,
				current_message: &mut self.current_message,
				data: &self.data
			},
			WsWriteHalf {
				stream: writer,
				last_sent_message_op: &mut self.last_sent_message_op,
				data: &self.data
			}
		)
	}
}

pub struct WebSocketServer {
	listener: TcpListener,
	options: WebSocketOptions
}

pub struct WebSocketHandle {
	stream: HttpConn,
	options: WebSocketOptions
}

#[asynchronous]
impl WebSocketHandle {
	async fn accept_websocket(self) -> Result<WebSocket> {
		struct WsServer {}

		let server = WsServer {};
		let stream = handle_upgrade(self.stream, &server)
			.timeout(self.options.handshake_timeout)
			.await
			.ok_or(WebSocketError::HandshakeTimeout)??;

		Ok(WebSocket::server(stream, &self.options))
	}
}

#[asynchronous(task)]
impl Task for WebSocketHandle {
	type Output = Result<WebSocket>;

	async fn run(self) -> Self::Output {
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
		let stream = HttpConn::new(socket);

		Ok(WebSocketHandle { stream, options: self.options })
	}

	pub async fn local_addr(&self) -> Result<SocketAddr> {
		self.listener.local_addr().await
	}
}
