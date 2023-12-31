use url::Url;
use xx_core::{error::*, task::Handle};
use xx_pulse::*;

use super::stream::FileStream;

pub struct Request {
	pub(crate) url: Url,
	pub(crate) start: Option<u64>,
	pub(crate) end: Option<u64>
}

impl Request {
	pub fn new(url: &str) -> Result<Self> {
		let this = Self {
			url: Url::parse(url).map_err(Error::map_as_invalid_input)?,
			start: None,
			end: None
		};

		if this.url.scheme() != "file" {
			return Err(Error::new(ErrorKind::InvalidInput, "Scheme must be 'file'"));
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

	#[async_fn]
	pub async fn run(&self) -> Result<FileStream> {
		FileStream::new(self).await
	}
}

impl Task for Request {
	type Output = Result<FileStream>;

	fn run(self, mut context: Handle<Context>) -> Result<FileStream> {
		context.run(FileStream::new(&self))
	}
}

pub fn get(url: &str) -> Result<Request> {
	Request::new(url)
}
