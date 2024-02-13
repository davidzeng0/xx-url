use std::time::Duration;

use http::Method;
use url::Url;
use xx_core::{
	coroutines::{with_context, Context, Task},
	error::*,
	macros::{duration, wrapper_functions},
	pointer::*
};
use xx_pulse::*;

use super::WebSocket;
use crate::{error::UrlError, http::transfer::Request, net::connection::IpStrategy};

const DEFAULT_MAX_MESSAGE_LENGTH: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct WebSocketOptions {
	pub(crate) handshake_timeout: Duration,
	pub(crate) max_message_length: u64,
	pub(crate) close_timeout: Duration
}

impl WebSocketOptions {
	pub fn new() -> Self {
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

	pub fn set_max_message_length(&mut self, max: u64) -> &mut Self {
		self.max_message_length = max;
		self
	}

	pub fn set_close_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.close_timeout = timeout;
		self
	}
}

pub struct WsRequest {
	pub(crate) inner: Request,
	pub(crate) options: WebSocketOptions
}

#[asynchronous]
impl WsRequest {
	wrapper_functions! {
		inner = self.inner;
		mut inner = self.inner;

		#[chain]
		pub fn header(&mut self, key: impl ToString, value: impl ToString) -> &mut Self;

		#[chain]
		pub fn set_port(&mut self, port: u16) -> &mut Self;

		#[chain]
		pub fn set_strategy(&mut self, strategy: IpStrategy) -> &mut Self;

		#[chain]
		pub fn set_timeout(&mut self, timeout: Duration) -> &mut Self;

		#[chain]
		pub fn set_recvbuf_size(&mut self, size: i32) -> &mut Self;

		#[chain]
		pub fn set_sendbuf_size(&mut self, size: i32) -> &mut Self;
	}

	pub async fn run(&mut self) -> Result<WebSocket> {
		WebSocket::new(self).await
	}

	pub fn set_handshake_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.options.set_handshake_timeout(timeout);
		self
	}

	pub fn set_max_message_length(&mut self, max: u64) -> &mut Self {
		self.options.set_max_message_length(max);
		self
	}

	pub fn set_close_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.options.set_close_timeout(timeout);
		self
	}
}

impl Task for WsRequest {
	type Output = Result<WebSocket>;

	fn run(mut self, context: Ptr<Context>) -> Result<WebSocket> {
		unsafe { with_context(context, WebSocket::new(&mut self)) }
	}
}

pub fn open(url: &str) -> Result<WsRequest> {
	let mut request = Request::new(
		Url::parse(url).map_err(Error::map_as_invalid_input)?,
		Method::GET
	);

	match request.url.scheme() {
		"ws" => (),
		"wss" => request.options.secure = true,
		_ => return Err(UrlError::InvalidScheme.new())
	}

	Ok(WsRequest { inner: request, options: WebSocketOptions::new() })
}
