use std::{
	io::{self, IoSlice, IoSliceMut},
	ops::{Deref, DerefMut},
	sync::Arc,
	time::{Duration, Instant}
};

use enumflags2::BitFlags;
use rustls::{ClientConfig, ClientConnection};
use x509_parser::prelude::*;
use xx_core::{
	async_std::io::*,
	debug,
	error::*,
	os::{
		poll::PollFlag,
		socket::{MessageFlag, Shutdown}
	},
	task::Handle
};
use xx_pulse::*;

use crate::{
	env::get_tls_client_config,
	net::connection::{self, ConnectOptions, Connection}
};

#[derive(Default)]
pub struct ConnectStats {
	pub stats: connection::ConnectStats,
	pub tls_connect: Duration
}

impl From<connection::ConnectStats> for ConnectStats {
	fn from(stats: connection::ConnectStats) -> Self {
		Self { stats, ..Default::default() }
	}
}

struct AsyncConnection {
	context: Handle<Context>,
	connection: Connection
}

impl Deref for AsyncConnection {
	type Target = Connection;

	fn deref(&self) -> &Connection {
		&self.connection
	}
}

impl DerefMut for AsyncConnection {
	fn deref_mut(&mut self) -> &mut Connection {
		&mut self.connection
	}
}

impl io::Read for AsyncConnection {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		self.connection
			.async_trait_read(buf, self.context)
			.map_err(|err| err.into())
	}

	fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
		self.connection
			.async_trait_read_vectored(bufs, self.context)
			.map_err(|err| err.into())
	}
}

impl io::Write for AsyncConnection {
	/* we set don't wait only on writes for error state */
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.context
			.run(self.connection.send(buf, MessageFlag::DontWait as u32))
			.map_err(|err| err.into())
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}

	fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
		self.context
			.run(
				self.connection
					.send_vectored(bufs, MessageFlag::DontWait as u32)
			)
			.map_err(|err| err.into())
	}
}

pub struct TlsConn {
	inner: AsyncConnection,
	tls: ClientConnection
}

fn make_client(options: &ConnectOptions, config: Arc<ClientConfig>) -> Result<ClientConnection> {
	let server_name = options
		.host()
		.try_into()
		.map_err(Error::invalid_data_error)?;

	ClientConnection::new(config, server_name).map_err(Error::invalid_data_error)
}

macro_rules! alias_func {
	($func: ident ($self: ident: $self_type: ty $(, $arg: ident: $type: ty)*) -> $return_type: ty) => {
		#[async_fn]
		pub async fn $func($self: $self_type $(, $arg: $type)*) -> $return_type {
			$self.inner.$func($($arg),*).await
		}
	}
}

#[async_fn]
impl TlsConn {
	alias_func!(shutdown(self: &Self, how: Shutdown) -> Result<()>);

	async fn tls_connect(&mut self, stats: &mut ConnectStats) -> Result<()> {
		let now = Instant::now();
		let mut eof = false;

		self.inner.context = get_context().await;

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

			let flags = self.inner.poll(flags).await?;

			if flags.intersects(PollFlag::Out) {
				if self.tls.write_tls(&mut self.inner)? == 0 {
					eof = true;
				}
			}

			if !handshaking && !eof {
				break;
			}

			if flags.intersects(PollFlag::In) {
				if self.tls.read_tls(&mut self.inner)? == 0 {
					eof = true;
				} else if let Err(err) = self.tls.process_new_packets() {
					let _ = self.tls.write_tls(&mut self.inner);

					return Err(Error::invalid_data_error(err));
				}
			}

			if handshaking && !self.tls.is_handshaking() && self.tls.wants_write() {
				continue;
			}

			match (eof, handshaking, self.tls.is_handshaking()) {
				(_, true, false) | (_, false, _) => break,
				(true, true, true) => {
					return Err(Error::new(
						ErrorKind::UnexpectedEof,
						"EOF while handshaking"
					))
				}
				(..) => ()
			}
		}

		let elapsed = now.elapsed();

		debug!(target: self, "== TLS connected using {:?} / {:?} ({:.3} ms)", self.tls.protocol_version().unwrap(), self.tls.negotiated_cipher_suite().unwrap(), elapsed.as_secs_f32() * 1000.0);

		if let Some((_, cert)) = self
			.tls
			.peer_certificates()
			.map(|certs| certs.first())
			.flatten()
			.map(|cert| X509Certificate::from_der(&cert.0).ok())
			.flatten()
		{
			debug!(target: self, "== Certificate: ");
			debug!(target: self, "==     Subject: {}", cert.subject());
			debug!(target: self, "==     Issuer : {}", cert.issuer());
			debug!(target: self, "==     Start  : {}", cert.validity().not_before);
			debug!(target: self, "==     Expire : {}", cert.validity().not_after);

			if let Ok(Some(alt)) = cert.subject_alternative_name() {
				for name in &alt.value.general_names {
					debug!(target: self, "==     Alt    : {}", name);
				}
			}
		}

		stats.tls_connect = elapsed;

		Ok(())
	}

	pub async fn connect_stats_config(
		options: &ConnectOptions, config: Arc<ClientConfig>
	) -> Result<(TlsConn, ConnectStats)> {
		let (connection, stats) = Connection::connect_stats(options).await?;

		let inner = AsyncConnection { context: unsafe { Handle::new_null() }, connection };
		let tls = make_client(options, config)?;
		let mut connection = TlsConn { inner, tls };
		let mut stats = stats.into();

		connection.tls_connect(&mut stats).await?;

		Ok((connection, stats))
	}

	pub async fn connect_config(
		options: &ConnectOptions, config: Arc<ClientConfig>
	) -> Result<TlsConn> {
		Ok(Self::connect_stats_config(options, config).await?.0)
	}

	pub async fn connect_stats(options: &ConnectOptions) -> Result<(TlsConn, ConnectStats)> {
		Ok(Self::connect_stats_config(options, get_tls_client_config()).await?)
	}

	pub async fn connect(options: &ConnectOptions) -> Result<TlsConn> {
		Ok(Self::connect_stats(options).await?.0)
	}

	async fn tls_read(
		&mut self, mut read: impl FnMut(&mut ClientConnection) -> io::Result<usize>
	) -> Result<usize> {
		match read(&mut self.tls) {
			Ok(0) => (),
			Ok(n) => return Ok(n),
			Err(err) if err.kind() == ErrorKind::WouldBlock => (),
			Err(err) => return Err(err.into())
		}

		loop {
			self.inner.context = get_context().await;

			if self.tls.read_tls(&mut self.inner)? == 0 {
				return Ok(0);
			}

			let state = self
				.tls
				.process_new_packets()
				.map_err(Error::invalid_data_error)?;

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
		self.inner.context = get_context().await;

		loop {
			let wrote = write(&mut self.tls)?;

			while self.tls.wants_write() {
				if self.tls.write_tls(&mut self.inner)? == 0 {
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

	pub fn has_peer_hungup(&mut self) -> Result<bool> {
		self.inner.has_peer_hungup()
	}

	pub async fn close(self) -> Result<()> {
		self.inner.connection.close().await
	}
}

#[async_trait_impl]
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

#[async_trait_impl]
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
