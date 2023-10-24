use std::io::SeekFrom;

use xx_async_runtime::Context;
use xx_core::{async_std::io::*, error::Result, read_into};
use xx_pulse::*;

use super::Request;

pub struct FileStream {
	file: File,
	start: u64,
	end: u64
}

#[async_fn]
impl FileStream {
	pub async fn new(request: &Request) -> Result<FileStream> {
		let mut file = File::open(request.url.path()).await?;

		let mut start = 0;
		/* file supports stream_len_fast, for now */
		let mut end = file.stream_len().await.unwrap();

		if let Some(pos) = request.start {
			start = pos;
		}

		if let Some(pos) = request.end {
			end = pos;
		}

		if start > end {
			end = start;
		}

		file.seek(SeekFrom::Start(start)).await?;

		Ok(Self { file, start, end })
	}

	fn len(&self) -> u64 {
		self.end - self.start
	}

	fn pos(&self) -> u64 {
		self.file.pos() - self.start
	}
}

#[async_trait_fn]
impl Read<Context> for FileStream {
	async fn async_read(&mut self, buf: &mut [u8]) -> Result<usize> {
		let remaining = (self.len() - self.stream_position().await.unwrap()) as usize;

		read_into!(buf, remaining);

		self.file.read(buf).await
	}
}

#[async_trait_fn]
impl Seek<Context> for FileStream {
	async fn async_seek(&mut self, seek: SeekFrom) -> Result<u64> {
		let pos = match seek {
			SeekFrom::Current(rel) => self.pos().wrapping_add_signed(rel),
			SeekFrom::Start(pos) => self.start.wrapping_add(pos),
			SeekFrom::End(pos) => self.end.wrapping_add_signed(pos)
		};

		self.file.seek(SeekFrom::Start(pos)).await
	}

	fn stream_len_fast(&self) -> bool {
		true
	}

	async fn async_stream_len(&mut self) -> Result<u64> {
		Ok(self.len())
	}

	fn stream_position_fast(&self) -> bool {
		true
	}

	async fn async_stream_position(&mut self) -> Result<u64> {
		Ok(self.pos())
	}
}
