use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use crypto::digest::Digest;
use crypto::sha1::Sha1;

use super::*;

pub struct Key {
	data: [u8; 16]
}

impl Key {
	pub fn new() -> Self {
		Self { data: rand::random() }
	}

	pub fn from(val: &str) -> Result<Self> {
		let mut data = [0u8; 16];

		STANDARD
			.decode_slice(val.as_bytes(), &mut data)
			.map_err(|_| WebSocketError::InvalidKey)?;

		Ok(Self { data })
	}

	#[allow(clippy::missing_panics_doc)]
	pub fn encode(&self, out: &mut [u8; 24]) {
		STANDARD.encode_slice(self.data, out).unwrap();
	}

	#[allow(clippy::missing_panics_doc)]
	pub fn accept(&self, out: &mut [u8; 28]) {
		let mut key = [0u8; 24];
		let mut sum = [0u8; 20];
		let mut sha = Sha1::new();

		self.encode(&mut key);

		sha.input(&key);
		sha.input_str(WEB_SOCKET_GUID);
		sha.result(&mut sum);

		STANDARD.encode_slice(sum, out).unwrap();
	}
}
