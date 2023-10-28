use std::str::from_utf8_unchecked;

use xx_core::{async_std::io::*, debug, error::*};
use xx_pulse::*;

use super::{consts::*, handshake::Key, WsRequest};
use crate::http::{stream::HttpStream, transfer::transfer};

#[async_fn]
pub async fn connect(request: &WsRequest) -> Result<BufReader<HttpStream>> {
	let mut key_bytes = [0u8; 24];
	let mut accept_bytes = [0u8; 28];

	let (key, accept) = {
		let rand = Key::new();

		rand.encode(&mut key_bytes);
		rand.accept(&mut accept_bytes);

		unsafe {
			(
				from_utf8_unchecked(&key_bytes),
				from_utf8_unchecked(&accept_bytes)
			)
		}
	};

	let timeout = request.options.handshake_timeout;

	let mut request = request.inner.clone();

	request.header("Connection", "Upgrade");
	request.header("Upgrade", "websocket");
	request.header("Sec-WebSocket-Version", WEB_SOCKET_VERSION);
	request.header("Sec-WebSocket-Key", key);

	let (response, reader) = match select(transfer(&request, None), sleep(timeout)).await {
		Select::First(conn, _) => conn?,
		Select::Second(..) => {
			return Err(Error::new(
				ErrorKind::TimedOut,
				"WebSocket connection timed out"
			))
		}
	};

	macro_rules! check_header {
		($header: literal, $value: expr, $message: literal) => {
			if !response.headers.get($header).is_some_and(|val| val.eq_ignore_ascii_case($value)) {
				debug!(target: &request, "== WebSocket connection refused");

				return Err(Error::new(ErrorKind::InvalidData, $message));
			}
		};
	}

	check_header!(
		"connection",
		"upgrade",
		"Expected value 'upgrade' for header 'Connection'"
	);

	check_header!(
		"upgrade",
		"websocket",
		"Expected value 'websocket' for header 'Upgrade'"
	);

	check_header!(
		"sec-websocket-accept",
		accept,
		"Mismatch for header 'Sec-WebSocket-Accept'"
	);

	Ok(reader)
}
