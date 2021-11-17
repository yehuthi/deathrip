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

/// Determines the limit of an axis for the image.
///
/// - The `base` parameter is the base URL of the image along with XYZ parameters (see section below), but with the
/// target axis parameter last and without a value (e.g. end with `x0-y0-z` to target the Z axis).
/// - The `num_workers` is the amount of simultaneous requests that will be made.
///
/// ## Base URL
///
/// An image base URL ends with `=` and then is appended with X, Y, and Z values in the format:
/// `x<X>-y<Y>-z<Z>`. The order of the axes is insignificant.
/// X and Y refer to position and Z refers to the resolution.
///
/// This function will send HEAD requests, incrementing an axis determined by the base URL,
/// and will return the highest value that succeeds.
async fn determine_limit(
	client: Arc<Client>,
	base: &str,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	// A variable dedicated for the result.
	// It's a `Result` that will be the minimal value that succeeds or an error if we encounter an
	// error (that isn't a client-error because we took the axis too far).
	let min_failure = Arc::new(RwLock::new(Ok::<usize, reqwest::Error>(usize::MAX)));
	// An atomic counter of the axis value. Threads read and increment it as they try higher axis values.
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

/// Determines the max zoom level for the image at the base URL.
pub async fn determine_max_zoom(
	client: Arc<Client>,
	base: &str,
	num_workers: usize,
) -> Result<usize, reqwest::Error> {
	determine_limit(client, &format!("{}x0-y0-z", base), num_workers).await
}

/// Determines the count of columns i.e. the amount of cells going across the image.
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

/// Determines the count of rows i.e. the amount of cells going along the image.
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

/// Determines the [rows](determine_rows) and [columns](determine_columns) of the image (in-parallel).
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

/// An error when fetching or processing an image.
#[derive(Debug)]
pub enum Error {
	/// Failure trying to fetch an image or metadata.
	HttpError(reqwest::Error),
	/// Failure trying to decode an image.
	ImageError(image::ImageError),
	/// Failure trying to determine the image's format.
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
