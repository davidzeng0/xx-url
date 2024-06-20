#![allow(unsafe_code)]

use std::io::{self, IoSlice, IoSliceMut};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::{ClientConfig, ClientConnection};
use x509_parser::prelude::*;
use xx_core::async_std::io::*;
use xx_core::coroutines::{get_context, scoped, Context};
use xx_core::enumflags2::BitFlags;
use xx_core::macros::wrapper_functions;
use xx_core::os::epoll::PollFlag;
use xx_core::os::socket::{MessageFlag, Shutdown};
use xx_core::{debug, trace};

use super::*;
use crate::net::connection::{self, ConnectOptions, Connection};

#[derive(Default, Clone, Copy)]
pub struct ConnectStats {
	pub stats: connection::ConnectStats,
	pub tls_connect: Duration
}

impl From<connection::ConnectStats> for ConnectStats {
	fn from(stats: connection::ConnectStats) -> Self {
		Self { stats, ..Default::default() }
	}
}

struct Adapter<'a> {
	connection: &'a mut Connection,
	context: &'a Context,
	flags: BitFlags<MessageFlag>
}

impl<'a> Adapter<'a> {
	fn new(connection: &'a mut Connection, context: &'a Context) -> Self {
		Self { connection, context, flags: BitFlags::default() }
	}
}

impl io::Read for Adapter<'_> {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		/* Safety: guaranteed by caller */
		unsafe { scoped(self.context, self.connection.recv(buf, self.flags)) }.map_err(Into::into)
	}

	fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
		/* Safety: guaranteed by caller */
		unsafe {
			scoped(
				self.context,
				self.connection.recv_vectored(bufs, self.flags)
			)
		}
		.map_err(Into::into)
	}
}

impl io::Write for Adapter<'_> {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		/* Safety: guaranteed by caller */
		unsafe { scoped(self.context, self.connection.send(buf, self.flags)) }.map_err(Into::into)
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}

	fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
		/* Safety: guaranteed by caller */
		unsafe {
			scoped(
				self.context,
				self.connection.send_vectored(bufs, self.flags)
			)
		}
		.map_err(Into::into)
	}
}

pub struct TlsConn {
	connection: Connection,
	tls: ClientConnection
}

#[asynchronous]
impl TlsConn {
	wrapper_functions! {
		inner = self.connection;

		pub fn has_peer_hungup(&self) -> Result<bool>;

		#[asynchronous]
		pub async fn poll(&mut self, flags: BitFlags<PollFlag>) -> Result<BitFlags<PollFlag>>;

		#[asynchronous]
		pub async fn shutdown(&mut self, how: Shutdown) -> Result<()>;

		#[asynchronous]
		pub async fn close(self) -> Result<()>;
	}

	async fn tls_connect(&mut self, stats: &mut ConnectStats) -> Result<()> {
		let now = Instant::now();
		let mut eof = false;

		/* Safety: we are in an async function */
		#[allow(clippy::multiple_unsafe_ops_per_block)]
		let mut adapter = unsafe { Adapter::new(&mut self.connection, get_context().await) };

		loop {
			let handshaking = self.tls.is_handshaking();

			/* poll to prevent hang when either read or write don't get through */
			let mut flags = BitFlags::default();

			if self.tls.wants_write() {
				flags |= PollFlag::Out;
			}

			if self.tls.wants_read() {
				flags |= PollFlag::In;
			}

			let flags = adapter.connection.poll(flags).await?;

			if flags.intersects(PollFlag::Out) && self.tls.write_tls(&mut adapter)? == 0 {
				eof = true;
			}

			if !handshaking && !eof {
				break;
			}

			if flags.intersects(PollFlag::In) {
				if self.tls.read_tls(&mut adapter)? == 0 {
					eof = true;
				} else if let Err(err) = self.tls.process_new_packets() {
					/* we don't want to wait for writes in error state */
					adapter.flags = MessageFlag::DontWait.into();

					let _ = self.tls.write_tls(&mut adapter);

					return Err(Error::new(err));
				}
			}

			if handshaking && !self.tls.is_handshaking() && self.tls.wants_write() {
				continue;
			}

			match (eof, handshaking, self.tls.is_handshaking()) {
				(_, true, false) | (_, false, _) => break,
				(true, true, true) => {
					return Err(fmt_error!("EOF during TLS handshake" @ ErrorKind::UnexpectedEof))
				}
				(..) => ()
			}
		}

		let elapsed = now.elapsed();

		debug!(
			target: &*self,
			"== TLS connected using {:?} / {:?} ({:.3} ms)",
			self.tls.protocol_version().unwrap(),
			self.tls.negotiated_cipher_suite().unwrap(),
			elapsed.as_secs_f32() * 1000.0
		);

		if let Some((_, cert)) = self
			.tls
			.peer_certificates()
			.and_then(|certs| certs.first())
			.and_then(|cert| X509Certificate::from_der(cert).ok())
		{
			trace!(target: &*self, "== Certificate: ");
			trace!(target: &*self, "::     Subject: {}", cert.subject());
			trace!(target: &*self, "::     Issuer : {}", cert.issuer());
			trace!(target: &*self, "::     Start  : {}", cert.validity().not_before);
			trace!(target: &*self, "::     Expire : {}", cert.validity().not_after);

			if let Ok(Some(alt)) = cert.subject_alternative_name() {
				for name in &alt.value.general_names {
					trace!(target: &*self, "::     Alt    : {}", name);
				}
			}
		}

		stats.tls_connect = elapsed;

		Ok(())
	}

	pub async fn connect_stats_config(
		options: &ConnectOptions<'_>, config: Arc<ClientConfig>
	) -> Result<(Self, ConnectStats)> {
		let server_name = options.host().to_string().try_into().map_err(Error::new)?;
		let tls = ClientConnection::new(config, server_name).map_err(Error::new)?;

		let (connection, stats) = Connection::connect_stats(options).await?;

		let mut connection = Self { connection, tls };
		let mut stats = stats.into();

		connection.tls_connect(&mut stats).await?;

		Ok((connection, stats))
	}

	pub async fn connect_config(
		options: &ConnectOptions<'_>, config: Arc<ClientConfig>
	) -> Result<Self> {
		Ok(Self::connect_stats_config(options, config).await?.0)
	}

	pub async fn connect_stats(options: &ConnectOptions<'_>) -> Result<(Self, ConnectStats)> {
		Self::connect_stats_config(options, get_tls_client_config().await).await
	}

	pub async fn connect(options: &ConnectOptions<'_>) -> Result<Self> {
		Ok(Self::connect_stats(options).await?.0)
	}

	async fn tls_read(
		&mut self, mut read: impl FnMut(&mut ClientConnection) -> io::Result<usize>
	) -> Result<usize> {
		match read(&mut self.tls) {
			Ok(0) => (),
			Ok(n) => return Ok(n),
			Err(err) if err.kind() == io::ErrorKind::WouldBlock => (),
			Err(err) => return Err(err.into())
		}

		/* Safety: we are in an async function */
		#[allow(clippy::multiple_unsafe_ops_per_block)]
		let mut adapter = unsafe { Adapter::new(&mut self.connection, get_context().await) };

		loop {
			if self.tls.read_tls(&mut adapter)? == 0 {
				return Ok(0);
			}

			let state = self.tls.process_new_packets().map_err(Error::new)?;

			if state.plaintext_bytes_to_read() == 0 {
				check_interrupt().await?;

				continue;
			}

			break Ok(read(&mut self.tls)?);
		}
	}

	pub async fn recv(&mut self, buf: &mut [u8]) -> Result<usize> {
		self.tls_read(move |tls| io::Read::read(&mut tls.reader(), buf))
			.await
	}

	pub async fn recv_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> Result<usize> {
		self.tls_read(|tls| io::Read::read_vectored(&mut tls.reader(), bufs))
			.await
	}

	async fn tls_write(
		&mut self, write: impl Fn(&mut ClientConnection) -> io::Result<usize>
	) -> Result<usize> {
		/* Safety: we are in an async function */
		#[allow(clippy::multiple_unsafe_ops_per_block)]
		let mut adapter = unsafe { Adapter::new(&mut self.connection, get_context().await) };

		loop {
			let wrote = write(&mut self.tls)?;

			while self.tls.wants_write() {
				if self.tls.write_tls(&mut adapter)? == 0 {
					return Ok(wrote);
				}

				if let Err(err) = check_interrupt().await {
					return if wrote == 0 { Err(err) } else { Ok(wrote) };
				}
			}

			if wrote != 0 {
				break Ok(wrote);
			}
		}
	}

	pub async fn send(&mut self, buf: &[u8]) -> Result<usize> {
		self.tls_write(|tls| io::Write::write(&mut tls.writer(), buf))
			.await
	}

	pub async fn send_vectored(&mut self, bufs: &[IoSlice<'_>]) -> Result<usize> {
		self.tls_write(|tls| io::Write::write_vectored(&mut tls.writer(), bufs))
			.await
	}
}

#[asynchronous]
impl Read for TlsConn {
	async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
		self.recv(buf).await
	}

	fn is_read_vectored(&self) -> bool {
		true
	}

	async fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> Result<usize> {
		self.recv_vectored(bufs).await
	}
}

#[asynchronous]
impl Write for TlsConn {
	async fn write(&mut self, buf: &[u8]) -> Result<usize> {
		self.send(buf).await
	}

	fn is_write_vectored(&self) -> bool {
		true
	}

	async fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> Result<usize> {
		self.send_vectored(bufs).await
	}
}
