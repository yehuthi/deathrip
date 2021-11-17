use std::{
	fmt::{self, Display},
	io::Cursor,
	sync::{
		atomic::{self, AtomicUsize},
		Arc,
	},
};

use image::{GenericImage, GenericImageView};
use itertools::Itertools;
use reqwest::Client;
use tokio::sync::{Mutex, RwLock};

/// A [`String`](String) buffer with a mutating tail.
#[derive(Debug, Hash, Default, Clone, PartialEq, PartialOrd, Eq, Ord)]
struct StringMutTail {
	/// The [`String`](String) value.
	url: String,
	/// The index of the tail. Text after it is considered the tail.
	tail_index: usize,
}

impl From<String> for StringMutTail {
	fn from(mut base: String) -> Self {
		let tail_index = base.len();
		base.reserve(10);
		Self {
			url: base,
			tail_index,
		}
	}
}

impl From<&str> for StringMutTail {
	fn from(base: &str) -> Self {
		Self::from(base.to_string())
	}
}

impl StringMutTail {
	/// Sets the [tail](StringMutTail::tail_index) to the given integer.
	fn with_tail_int(&mut self, integer: impl itoa::Integer) -> &str {
		self.url.truncate(self.tail_index);
		itoa::fmt(&mut self.url, integer).unwrap();
		&self.url
	}
}

async fn determine_limit(
	client: Arc<Client>,
	base: &str,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	let min_failure = Arc::new(RwLock::new(Ok::<usize, reqwest::Error>(usize::MAX)));
	let i = Arc::new(AtomicUsize::new(1));

	let workers = (0..num_workers).map(|_| {
		let mut base = StringMutTail::from(base);
		let client = Arc::clone(&client);
		let i = Arc::clone(&i);
		let min_failure = Arc::clone(&min_failure);
		tokio::spawn(async move {
			loop {
				let level = i.fetch_add(1, atomic::Ordering::SeqCst);
				let response = client
					.head(base.with_tail_int(level))
					.send()
					.await
					.and_then(|r| r.error_for_status());
				match response {
					Ok(_) => {}
					Err(e) if e.status().map_or(false, |c| c.is_client_error()) => {
						let mut current_result = min_failure.write().await;
						match *current_result {
							Ok(previous_level) if level <= previous_level => {
								*current_result = Ok(level);
							}
							Ok(_) => {}
							Err(_) => {}
						}
						break;
					}
					Err(e) => {
						*min_failure.write().await = Err(e);
						break;
					}
				}
			}
		})
	});

	futures::future::try_join_all(workers).await.unwrap();
	Arc::try_unwrap(min_failure)
		.unwrap()
		.into_inner()
		.map(|l| l - 1)
}

pub async fn determine_max_zoom(
	client: Arc<Client>,
	base: &str,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	determine_limit(client, &format!("{}x0-y0-z", base), num_workers).await
}

pub async fn determine_columns(
	client: Arc<Client>,
	base: &str,
	zoom: usize,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	let base = format!("{}z{}-y0-x", base, zoom);
	determine_limit(client, &base, num_workers)
		.await
		.map(|c| c + 1)
}

pub async fn determine_rows(
	client: Arc<Client>,
	base: &str,
	zoom: usize,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	let base = format!("{}z{}-x0-y", base, zoom);
	determine_limit(client, &base, num_workers)
		.await
		.map(|c| c + 1)
}

pub async fn determine_dimensions(
	client: Arc<Client>,
	base: &str,
	zoom: usize,
	num_workers_half: usize,
) -> Result<(usize, usize), reqwest::Error> {
	tokio::try_join!(
		determine_columns(Arc::clone(&client), base, zoom, num_workers_half),
		determine_rows(client, base, zoom, num_workers_half)
	)
}

#[derive(Debug)]
pub enum Error {
	HttpError(reqwest::Error),
	ImageError(image::ImageError),
	ImageFormatGuessError(std::io::Error),
}

impl Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Error::HttpError(e) => write!(f, "HTTP error: {}", e),
			Error::ImageError(e) => write!(f, "Image processing error: {}", e),
			Error::ImageFormatGuessError(e) => write!(f, "Image format inference error: {}", e),
		}
	}
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
	fn from(e: reqwest::Error) -> Self {
		Self::HttpError(e)
	}
}

impl From<image::ImageError> for Error {
	fn from(e: image::ImageError) -> Self {
		Self::ImageError(e)
	}
}

pub async fn rip(
	client: Arc<Client>,
	base: &str,
	num_workers_half: usize,
) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, Error> {
	let zoom = determine_max_zoom(Arc::clone(&client), base, num_workers_half * 2).await?;
	let dims_task = {
		let client = Arc::clone(&client);
		async {
			determine_dimensions(client, base, zoom, num_workers_half)
				.await
				.map_err(Error::HttpError)
		}
	};
	let fetch_cell_client = Arc::clone(&client);
	let fetch_cell = |(x, y): (usize, usize)| {
		let client = Arc::clone(&fetch_cell_client);
		async move {
			let data = client
				.get(format!("{}x{}-y{}-z{}", base, x, y, zoom))
				.send()
				.await?
				.error_for_status()?
				.bytes()
				.await?;
			image::io::Reader::new(Cursor::new(data))
				.with_guessed_format()
				.map_err(Error::ImageFormatGuessError)?
				.decode()
				.map_err(Error::ImageError)
		}
	};
	let head_task = fetch_cell((0, 0));
	let ((columns, rows), head) = tokio::try_join!(dims_task, head_task)?;
	let (tile_width, tile_height) = head.dimensions();

	let mut image = image::ImageBuffer::new(columns as u32 * tile_width, rows as u32 * tile_height);
	image.copy_from(&head, 0, 0)?;

	let image = Arc::new(Mutex::new(image));
	let cells = (0..columns).cartesian_product(0..rows).skip(1);
	futures::future::try_join_all(cells.map(|(x, y)| {
		let image = Arc::clone(&image);
		async move {
			let cell = fetch_cell((x, y)).await?;
			image
				.lock()
				.await
				.copy_from(&cell, x as u32 * tile_width, y as u32 * tile_height)?;
			Ok::<(), Error>(())
		}
	}))
	.await
	.unwrap();

	Ok(Arc::try_unwrap(image).unwrap().into_inner())
}
