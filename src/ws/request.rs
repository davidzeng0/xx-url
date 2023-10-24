use std::{
	ops::{Deref, DerefMut},
	time::Duration
};

use http::Method;
use url::Url;
use xx_async_runtime::Context;
use xx_core::{error::*, task::*};
use xx_pulse::*;

use super::WebSocket;
use crate::http::transfer::Request;

const DEFAULT_MAX_MESSAGE_LENGTH: u64 = 128 * 1024 * 1024;

pub(crate) struct Options {
	pub(crate) handshake_timeout: Duration,
	pub(crate) max_message_length: u64,
	pub(crate) close_timeout: Duration
}

impl Options {
	pub(crate) fn new() -> Self {
		Self {
			handshake_timeout: Duration::from_secs(60),
			max_message_length: DEFAULT_MAX_MESSAGE_LENGTH,
			close_timeout: Duration::from_secs(30)
		}
	}
}

pub struct WsRequest {
	pub(crate) inner: Request,
	pub(crate) options: Options
}

#[async_fn]
impl WsRequest {
	pub async fn run(&mut self) -> Result<WebSocket> {
		WebSocket::new(self).await
	}

	pub fn set_max_message_length(&mut self, max: u64) -> &mut Self {
		self.options.max_message_length = max;
		self
	}

	pub fn set_close_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.options.close_timeout = timeout;
		self
	}
}

impl AsyncTask<Context, Result<WebSocket>> for WsRequest {
	fn run(mut self, mut context: Handle<Context>) -> Result<WebSocket> {
		context.run(WebSocket::new(&mut self))
	}
}

impl Deref for WsRequest {
	type Target = Request;

	fn deref(&self) -> &Request {
		&self.inner
	}
}

impl DerefMut for WsRequest {
	fn deref_mut(&mut self) -> &mut Request {
		&mut self.inner
	}
}

pub fn get(url: &str) -> Result<WsRequest> {
	let mut request = Request::new(
		Url::parse(url).map_err(Error::invalid_input_error)?,
		Method::GET
	);

	match request.url.scheme() {
		"ws" => (),
		"wss" => request.options.secure = true,
		_ => {
			return Err(Error::new(
				ErrorKind::InvalidInput,
				"Scheme must be 'ws' or 'wss'"
			))
		}
	}

	Ok(WsRequest { inner: request, options: Options::new() })
}
