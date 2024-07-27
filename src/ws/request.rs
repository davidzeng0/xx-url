use super::*;
use crate::net::conn::IpStrategy;

const DEFAULT_MAX_MESSAGE_LENGTH: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct WebSocketOptions {
	pub(super) handshake_timeout: Duration,
	pub(super) max_message_length: usize,
	pub(super) close_timeout: Duration
}

impl WebSocketOptions {
	#[must_use]
	pub const fn new() -> Self {
		Self {
			handshake_timeout: duration!(1 m),
			max_message_length: DEFAULT_MAX_MESSAGE_LENGTH,
			close_timeout: duration!(0.5 m)
		}
	}

	pub fn set_handshake_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.handshake_timeout = timeout;
		self
	}

	pub fn set_max_message_length(&mut self, max: usize) -> &mut Self {
		self.max_message_length = max;
		self
	}

	pub fn set_close_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.close_timeout = timeout;
		self
	}
}

impl Default for WebSocketOptions {
	fn default() -> Self {
		Self::new()
	}
}

pub struct WsRequest {
	pub(super) inner: Request,
	pub(super) options: WebSocketOptions
}

#[asynchronous]
impl WsRequest {
	wrapper_functions! {
		inner = self.inner;
		mut inner = self.inner;

		#[allow(clippy::impl_trait_in_params)]
		pub fn header(&mut self, key: impl TryIntoHeaderName, value: impl TryIntoHeaderValue) -> &mut Self;

		pub fn set_port(&mut self, port: u16) -> &mut Self;

		pub fn set_strategy(&mut self, strategy: IpStrategy) -> &mut Self;

		pub fn set_timeout(&mut self, timeout: Duration) -> &mut Self;

		pub fn set_recvbuf_size(&mut self, size: i32) -> &mut Self;

		pub fn set_sendbuf_size(&mut self, size: i32) -> &mut Self;

		#[allow(clippy::impl_trait_in_params)]
		pub fn payload(&mut self, payload: impl Into<Payload>) -> &mut Self;
	}

	pub async fn run(&mut self) -> Result<WebSocket> {
		WebSocket::new(self).await
	}

	pub fn set_handshake_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.options.set_handshake_timeout(timeout);
		self
	}

	pub fn set_max_message_length(&mut self, max: usize) -> &mut Self {
		self.options.set_max_message_length(max);
		self
	}

	pub fn set_close_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.options.set_close_timeout(timeout);
		self
	}
}

#[asynchronous(task)]
impl Task for WsRequest {
	type Output = Result<WebSocket>;

	async fn run(mut self) -> Self::Output {
		WebSocket::new(&mut self).await
	}
}

#[allow(clippy::impl_trait_in_params)]
pub fn open(url: impl AsRef<str>) -> Result<WsRequest> {
	let request = RequestBase::new(url, |scheme| matches!(scheme, "ws" | "wss"));
	let mut inner = Request::new(request, Method::GET);

	if let Some(url) = inner.request.url() {
		if url.scheme() == "wss" {
			inner.options.secure = true;
		}
	}

	Ok(WsRequest { inner, options: WebSocketOptions::new() })
}
