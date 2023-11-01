use xx_core::{async_std::io::*, error::Result, os::socket::Shutdown, read_wrapper, write_wrapper};
use xx_pulse::*;

use crate::{net::connection::Connection, tls::connection::TlsConn};

#[async_trait]
pub(crate) trait Inner: Read + Write {
	async fn shutdown(&mut self, how: Shutdown) -> Result<()>;
}

#[async_trait_impl]
impl Inner for Connection {
	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		Connection::shutdown(self, how).await
	}
}

#[async_trait_impl]
impl Inner for TlsConn {
	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		TlsConn::shutdown(self, how).await
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
		self.inner.shutdown(how).await
	}
}

impl Read for HttpStream {
	read_wrapper! {
		inner = inner;
		mut inner = inner;
	}
}

impl Write for HttpStream {
	write_wrapper! {
		inner = inner;
		mut inner = inner;
	}
}

impl Split for HttpStream {}
