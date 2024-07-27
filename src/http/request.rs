use xx_core::coroutines::Task;
use xx_core::macros::wrapper_functions;

use super::*;
use crate::net::conn::IpStrategy;

pub struct HttpRequest {
	pub(super) inner: Request
}

#[asynchronous]
impl HttpRequest {
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

	pub async fn run(&mut self) -> Result<Response> {
		Response::fetch(self).await
	}
}

#[asynchronous(task)]
impl Task for HttpRequest {
	type Output = Result<Response>;

	async fn run(mut self) -> Self::Output {
		Response::fetch(&mut self).await
	}
}

fn new_request(url: impl AsRef<str>, method: Method) -> HttpRequest {
	let request = RequestBase::new(url, |scheme| matches!(scheme, "http" | "https"));
	let mut inner = Request::new(request, method);

	if let Some(url) = inner.request.url() {
		if url.scheme() == "https" {
			inner.options.secure = true;
		}
	}

	HttpRequest { inner }
}

#[must_use]
#[allow(clippy::impl_trait_in_params)]
pub fn get(url: impl AsRef<str>) -> HttpRequest {
	new_request(url, Method::GET)
}

#[must_use]
#[allow(clippy::impl_trait_in_params)]
pub fn post(url: impl AsRef<str>, payload: impl Into<Payload>) -> HttpRequest {
	let mut request = new_request(url, Method::POST);

	request.payload(payload);
	request
}
