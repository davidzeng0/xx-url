use base64::{engine::general_purpose::STANDARD, Engine};
use crypto::{digest::Digest, sha1::Sha1};

use super::consts::WEB_SOCKET_GUID;

pub struct Key {
	data: [u8; 16]
}

impl Key {
	pub fn new() -> Self {
		Self { data: rand::random() }
	}

	pub fn encode(&self, out: &mut [u8; 24]) {
		STANDARD.encode_slice(self.data, out).unwrap();
	}

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
