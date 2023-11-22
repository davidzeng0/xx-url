use enumflags2::BitFlags;
use xx_core::{
	async_std::io::*,
	error::Result,
	os::{poll::PollFlag, socket::Shutdown},
	read_wrapper, wrapper_functions, write_wrapper
};
use xx_pulse::*;

use crate::{net::connection::Connection, tls::connection::TlsConn};

#[async_trait]
pub(crate) trait Inner: Read + Write {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

	async fn shutdown(&mut self, how: Shutdown) -> Result<()>;
}

#[async_trait_impl]
impl Inner for StreamSocket {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		StreamSocket::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		StreamSocket::shutdown(self, how).await
	}
}

#[async_trait_impl]
impl Inner for Connection {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		Connection::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		Connection::shutdown(self, how).await
	}
}

#[async_trait_impl]
impl Inner for TlsConn {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		TlsConn::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		TlsConn::shutdown(self, how).await
	}
}

pub struct HttpStream {
	inner: Box<dyn Inner>
}

#[async_fn]
impl HttpStream {
	wrapper_functions! {
		inner = self.inner;

		#[async_fn]
		pub async fn shutdown(&mut self, how: Shutdown) -> Result<()>;

		#[async_fn]
		pub async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;
	}

	pub(crate) fn new(inner: impl Inner + 'static) -> Self {
		Self { inner: Box::new(inner) }
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
