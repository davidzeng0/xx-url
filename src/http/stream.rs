use std::io::{IoSlice, IoSliceMut};

use xx_async_runtime::Context;
use xx_core::{async_std::io::*, error::Result, os::socket::Shutdown};
use xx_pulse::*;

use crate::{net::connection::Connection, tls::connection::TlsConn};

#[async_trait_fn]
pub(crate) trait Inner: Read<Context> + Write<Context> {
	async fn async_shutdown(&mut self, how: Shutdown) -> Result<()>;
}

#[async_trait_fn]
impl Inner for Connection {
	async fn async_shutdown(&mut self, how: Shutdown) -> Result<()> {
		self.shutdown(how).await
	}
}

#[async_trait_fn]
impl Inner for TlsConn {
	async fn async_shutdown(&mut self, how: Shutdown) -> Result<()> {
		self.shutdown(how).await
	}
}

pub struct HttpStream {
	inner: Box<dyn Inner>
}

#[async_fn]
impl HttpStream {
	pub(crate) fn new(inner: impl Inner + 'static) -> Self {
		Self { inner: Box::new(inner) }
	}

	pub async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		self.inner.async_shutdown(how, get_context().await)
	}
}

impl Read<Context> for HttpStream {
	#[async_trait_fn]
	async fn async_read(&mut self, buf: &mut [u8]) -> Result<usize> {
		self.inner.as_mut().read(buf).await
	}

	fn is_read_vectored(&self) -> bool {
		self.inner.is_write_vectored()
	}

	#[async_trait_fn]
	async fn async_read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> Result<usize> {
		self.inner.as_mut().read_vectored(bufs).await
	}
}

impl Write<Context> for HttpStream {
	#[async_trait_fn]
	async fn async_write(&mut self, buf: &[u8]) -> Result<usize> {
		self.inner.as_mut().write(buf).await
	}

	fn is_write_vectored(&self) -> bool {
		self.inner.is_write_vectored()
	}

	#[async_trait_fn]
	async fn async_write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> Result<usize> {
		self.inner.as_mut().write_vectored(bufs).await
	}

	#[async_trait_fn]
	async fn async_flush(&mut self) -> Result<()> {
		self.inner.as_mut().flush().await
	}
}

impl Split<Context> for HttpStream {}
