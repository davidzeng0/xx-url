use url::Url;
use xx_core::{
	coroutines::{with_context, Context, Task},
	error::*,
	pointer::*
};
use xx_pulse::*;

use super::stream::FileStream;
use crate::error::UrlError;

pub struct Request {
	pub(crate) url: Url,
	pub(crate) start: Option<u64>,
	pub(crate) end: Option<u64>
}

impl Request {
	pub fn new(url: &str) -> Result<Self> {
		let this = Self {
			url: Url::parse(url).map_err(|_| UrlError::InvalidUrl.new())?,
			start: None,
			end: None
		};

		if this.url.scheme() != "file" {
			return Err(UrlError::InvalidScheme.new());
		}

		Ok(this)
	}

	pub fn start(&mut self, start: u64) -> &mut Self {
		self.start = Some(start);
		self
	}

	pub fn end(&mut self, end: u64) -> &mut Self {
		self.end = Some(end);
		self
	}

	#[asynchronous]
	pub async fn run(&self) -> Result<FileStream> {
		FileStream::new(self).await
	}
}

impl Task for Request {
	type Output = Result<FileStream>;

	fn run(self, context: Ptr<Context>) -> Result<FileStream> {
		unsafe { with_context(context, FileStream::new(&self)) }
	}
}

pub fn get(url: &str) -> Result<Request> {
	Request::new(url)
}
