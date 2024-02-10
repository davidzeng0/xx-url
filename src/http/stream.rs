use enumflags2::BitFlags;
use xx_core::{
	async_std::io::*,
	error::Result,
	macros::wrapper_functions,
	os::{poll::PollFlag, socket::Shutdown},
	read_wrapper, write_wrapper
};
use xx_pulse::*;

use crate::{net::connection::Connection, tls::connection::TlsConn};

#[asynchronous]
pub(crate) trait Inner: Read + Write {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

	async fn shutdown(&mut self, how: Shutdown) -> Result<()>;
}

#[asynchronous]
impl Inner for StreamSocket {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		StreamSocket::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		StreamSocket::shutdown(self, how).await
	}
}

#[asynchronous]
impl Inner for Connection {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		Connection::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		Connection::shutdown(self, how).await
	}
}

#[asynchronous]
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

#[asynchronous]
impl HttpStream {
	wrapper_functions! {
		inner = self.inner;

		#[asynchronous]
		pub async fn shutdown(&mut self, how: Shutdown) -> Result<()>;

		#[asynchronous]
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

unsafe impl SimpleSplit for HttpStream {}
