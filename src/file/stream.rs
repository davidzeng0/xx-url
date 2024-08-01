use std::io::SeekFrom;

use xx_core::async_std::io::*;
use xx_pulse::fs::File;

use super::*;

pub struct FileStream {
	file: File,
	start: u64,
	end: u64
}

#[asynchronous]
impl FileStream {
	pub async fn new(request: &mut Request) -> Result<Self> {
		let mut file = File::open(request.inner.finalize()?.path()).await?;

		let mut start = 0;
		let mut end = file.stream_len().await?;

		if let Some(pos) = request.start {
			start = pos.min(end);
		}

		if let Some(pos) = request.end {
			end = pos;
		}

		end = end.max(start);
		file.seek(SeekFrom::Start(start)).await?;

		Ok(Self { file, start, end })
	}

	#[must_use]
	const fn len(&self) -> u64 {
		#[allow(clippy::arithmetic_side_effects)]
		(self.end - self.start)
	}

	#[must_use]
	const fn pos(&self) -> u64 {
		#[allow(clippy::arithmetic_side_effects)]
		(self.file.pos() - self.start)
	}
}

#[asynchronous]
impl Read for FileStream {
	async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
		#[allow(clippy::arithmetic_side_effects)]
		let remaining = self.len() - self.pos();

		read_into!(buf, remaining.try_into().unwrap_or(usize::MAX));

		self.file.read(buf).await
	}
}

#[asynchronous]
impl Seek for FileStream {
	async fn seek(&mut self, seek: SeekFrom) -> Result<u64> {
		let pos = match seek {
			SeekFrom::Current(rel) => self.pos().checked_add_signed(rel),
			SeekFrom::Start(pos) => self.start.checked_add(pos),
			SeekFrom::End(pos) => self.end.checked_add_signed(pos)
		}
		.unwrap();

		let pos = pos.clamp(self.start, self.end);

		self.file.seek(SeekFrom::Start(pos)).await
	}

	fn stream_len_fast(&self) -> bool {
		true
	}

	async fn stream_len(&mut self) -> Result<u64> {
		Ok(self.len())
	}

	fn stream_position_fast(&self) -> bool {
		true
	}

	async fn stream_position(&mut self) -> Result<u64> {
		Ok(self.pos())
	}
}
