#![allow(unreachable_pub)]

use xx_core::enumflags2::BitFlags;
use xx_core::macros::wrapper_functions;
use xx_core::os::epoll::PollFlag;
use xx_core::os::socket::Shutdown;
use xx_pulse::net::*;

use super::*;
use crate::net::conn::Conn;
use crate::tls::conn::*;

#[asynchronous(impl(mut, box))]
pub trait ConnExtra {
	async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

	async fn shutdown(&mut self, how: Shutdown) -> Result<()>;
}

macro_rules! impl_extra {
	($type:ident $($generics:tt)*) => {
		#[asynchronous]
		#[allow(single_use_lifetimes)]
		impl $($generics)* ConnExtra for $type $($generics)* {
			async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>> {
				Self::poll(self, flags).await
			}

			async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
				Self::shutdown(self, how).await
			}
		}
	};
}

macro_rules! impl_half {
	($type:ident $($generics:tt)*) => {
		impl_extra!($type $($generics)*);

		#[allow(single_use_lifetimes)]
		impl $($generics)* ReadHalf for $type $($generics)* {}

		#[allow(single_use_lifetimes)]
		impl $($generics)* WriteHalf for $type $($generics)* {}
	};
}

macro_rules! impl_conn {
	($type:ident) => {
		impl_extra!($type);

		#[asynchronous]
		impl Connection for $type {
			fn try_split(&mut self) -> Result<(HttpConnReadHalf<'_>, HttpConnWriteHalf<'_>)> {
				let (reader, writer) = SplitMut::try_split(self)?;

				Ok((Box::new(reader), Box::new(writer)))
			}
		}
	};
}

macro_rules! extra_wrapper {
	($type:ty) => {
		impl ConnExtra for $type {
			wrapper_functions! {
				inner = self.inner;

				#[asynchronous(traitfn)]
				async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

				#[asynchronous(traitfn)]
				async fn shutdown(&mut self, how: Shutdown) -> Result<()>;
			}
		}
	};
}

macro_rules! impl_bufread {
	($type:ty) => {
		impl<T: ConnExtra + ?Sized> ConnExtra for $type {
			wrapper_functions! {
				inner = self.inner_mut();

				#[asynchronous(traitfn)]
				async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

				#[asynchronous(traitfn)]
				async fn shutdown(&mut self, how: Shutdown) -> Result<()>;
			}
		}
	};
}

impl_bufread!(BufReader<T>);
impl_bufread!(BufReadHalf<'_, T>);

pub trait ReadHalf: Read + ConnExtra {}

pub trait WriteHalf: Write + ConnExtra {}

#[asynchronous(impl(mut, box))]
pub trait Connection: ConnExtra + Read + Write {
	fn try_split(&mut self) -> Result<(HttpConnReadHalf<'_>, HttpConnWriteHalf<'_>)>;
}

impl_half!(SocketHalf<'a>);
impl_conn!(StreamSocket);

impl_conn!(Conn);

impl_extra!(TlsReadHalf<'a>);
impl_extra!(TlsWriteHalf<'a>);

impl WriteHalf for TlsWriteHalf<'_> {}
impl ReadHalf for TlsReadHalf<'_> {}

impl_conn!(TlsConn);

pub struct HttpConn {
	inner: Box<dyn Connection + Send + Sync>
}

#[asynchronous]
impl HttpConn {
	pub(crate) fn new(inner: impl Connection + Send + Sync + 'static) -> Self {
		Self { inner: Box::new(inner) }
	}
}

impl Read for HttpConn {
	read_wrapper! {
		inner = inner;
		mut inner = inner;
	}
}

impl Write for HttpConn {
	write_wrapper! {
		inner = inner;
		mut inner = inner;
	}
}

extra_wrapper!(HttpConn);

pub type HttpConnReadHalf<'a> = Box<dyn ReadHalf + Send + Sync + 'a>;
pub type HttpConnWriteHalf<'a> = Box<dyn WriteHalf + Send + Sync + 'a>;

impl SplitMut for HttpConn {
	type Reader<'a> = HttpConnReadHalf<'a>;
	type Writer<'a> = HttpConnWriteHalf<'a>;

	fn try_split(&mut self) -> Result<(Self::Reader<'_>, Self::Writer<'_>)> {
		self.inner.try_split()
	}
}
