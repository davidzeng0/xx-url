use xx_core::coroutines::Task;

use super::*;

pub struct Request {
	pub(super) inner: RequestBase,
	pub(super) start: Option<u64>,
	pub(super) end: Option<u64>
}

impl Request {
	#[must_use]
	#[allow(clippy::impl_trait_in_params)]
	pub fn new(url: impl AsRef<str>) -> Self {
		Self {
			inner: RequestBase::new(url, |scheme| scheme == "file"),
			start: None,
			end: None
		}
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
	pub async fn run(&mut self) -> Result<FileStream> {
		FileStream::new(self).await
	}
}

#[asynchronous(task)]
impl Task for Request {
	type Output = Result<FileStream>;

	async fn run(mut self) -> Self::Output {
		FileStream::new(&mut self).await
	}
}

#[must_use]
#[allow(clippy::impl_trait_in_params)]
pub fn get(url: impl AsRef<str>) -> Request {
	Request::new(url)
}
