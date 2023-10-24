use std::ops::{Deref, DerefMut};

use http::Method;
use url::Url;
use xx_async_runtime::Context;
use xx_core::{error::*, task::*};
use xx_pulse::*;

use super::{transfer::Request, Response};
pub struct HttpRequest {
	inner: Request
}

#[async_fn]
impl HttpRequest {
	pub async fn run(&self) -> Result<Response> {
		Response::fetch(self).await
	}
}

impl AsyncTask<Context, Result<Response>> for HttpRequest {
	fn run(self, mut context: Handle<Context>) -> Result<Response> {
		context.run(Response::fetch(&self))
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
	let mut request = Request::new(Url::parse(url).map_err(Error::invalid_input_error)?, method);

	match request.url.scheme() {
		"http" => (),
		"https" => request.options.secure = true,
		_ => {
			return Err(Error::new(
				ErrorKind::InvalidInput,
				"Scheme must be either 'http' or 'https'"
			))
		}
	}

	Ok(HttpRequest { inner: request })
}

pub fn get(url: &str) -> Result<HttpRequest> {
	new_request(url, Method::GET)
}
