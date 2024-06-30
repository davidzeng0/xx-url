use xx_core::enumflags2::BitFlags;
use xx_core::macros::wrapper_functions;
use xx_core::os::epoll::PollFlag;
use xx_core::os::socket::Shutdown;

use super::*;
use crate::net::connection::Connection;
use crate::tls::connection::TlsConn;

#[asynchronous]
#[allow(unreachable_pub)]
pub trait Inner: Read + Write {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

	async fn shutdown(&mut self, how: Shutdown) -> Result<()>;
}

#[asynchronous]
impl Inner for StreamSocket {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		Self::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		Self::shutdown(self, how).await
	}
}

#[asynchronous]
impl Inner for Connection {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		Self::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		Self::shutdown(self, how).await
	}
}

#[asynchronous]
impl Inner for TlsConn {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
		Self::poll(self, flags).await
	}

	async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
		Self::shutdown(self, how).await
	}
}

pub struct HttpStream {
	inner: Box<dyn Inner + Send + Sync>
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

	pub(crate) fn new(inner: impl Inner + Send + Sync + 'static) -> Self {
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
