use std::time::Duration;

use xx_core::{
	coroutines::{with_context, Context, Task},
	macros::wrapper_functions,
	pointer::*
};

use super::*;
use crate::net::connection::IpStrategy;

pub struct HttpRequest {
	pub(super) inner: Request
}

#[asynchronous]
impl HttpRequest {
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

	pub async fn run(&self) -> Result<Response> {
		Response::fetch(self).await
	}
}

impl Task for HttpRequest {
	type Output = Result<Response>;

	fn run(self, context: Ptr<Context>) -> Result<Response> {
		unsafe { with_context(context, Response::fetch(&self)) }
	}
}

fn new_request(url: &str, method: Method) -> Result<HttpRequest> {
	let mut request = Request::new(
		Url::parse(url).map_err(|_| UrlError::InvalidUrl.new())?,
		method
	);

	match request.url.scheme() {
		"http" => (),
		"https" => request.options.secure = true,
		_ => return Err(UrlError::InvalidScheme.new())
	}

	Ok(HttpRequest { inner: request })
}

pub fn get(url: &str) -> Result<HttpRequest> {
	new_request(url, Method::GET)
}
