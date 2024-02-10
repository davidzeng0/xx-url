use std::ops::{Deref, DerefMut};

use http::Method;
use url::Url;
use xx_core::{
	coroutines::{with_context, Context, Task},
	error::*,
	pointer::*
};
use xx_pulse::*;

use super::{transfer::Request, Response};
use crate::error::UrlError;
pub struct HttpRequest {
	inner: Request
}

#[asynchronous]
impl HttpRequest {
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

impl Deref for HttpRequest {
	type Target = Request;

	fn deref(&self) -> &Request {
		&self.inner
	}
}

impl DerefMut for HttpRequest {
	fn deref_mut(&mut self) -> &mut Request {
		&mut self.inner
	}
}

fn new_request(url: &str, method: Method) -> Result<HttpRequest> {
	let mut request = Request::new(
		Url::parse(url).map_err(Error::map_as_invalid_input)?,
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
