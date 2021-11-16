use std::{
	fmt::{self, Display},
	io::Cursor,
	sync::Arc,
};

use image::{GenericImage, GenericImageView};
use itertools::Itertools;
use reqwest::Client;
use tokio::sync::Mutex;

async fn determine_limit(
	client: &Client,
	mut base: String,
	params: &str,
) -> Result<usize, reqwest::Error> {
	let mut level = 0;
	base.reserve(10);
	base.push_str(params);
	let axis_index = base.len() - 1;

	loop {
		let response = client.head(&base).send().await?;
		if response.status().is_success() {
			level += 1;
			let next_level = level + 1;
			base.truncate(axis_index);
			itoa::fmt(&mut base, next_level).unwrap();
		} else {
			break;
		}
	}

	Ok(level)
}

pub async fn determine_max_zoom(client: &Client, base: String) -> Result<usize, reqwest::Error> {
	determine_limit(client, base, "x0-y0-z1").await
}

pub async fn determine_columns(
	client: &Client,
	base: String,
	zoom: usize,
) -> Result<usize, reqwest::Error> {
	let params = format!("z{}-y0-x1", zoom);
	determine_limit(client, base, &params).await.map(|c| c + 1)
}

pub async fn determine_rows(
	client: &Client,
	base: String,
	zoom: usize,
) -> Result<usize, reqwest::Error> {
	let params = format!("z{}-x0-y1", zoom);
	determine_limit(client, base, &params).await.map(|r| r + 1)
}

pub async fn determine_dimensions(
	client: &Client,
	base: String,
	zoom: usize,
) -> Result<(usize, usize), reqwest::Error> {
	tokio::try_join!(
		determine_columns(client, base.clone(), zoom),
		determine_rows(client, base, zoom)
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
	client: &reqwest::Client,
	base: String,
) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, Error> {
	let zoom = determine_max_zoom(client, base.clone()).await?;
	let dims_task = async {
		determine_dimensions(client, base.clone(), zoom)
			.await
			.map_err(Error::HttpError)
	};
	let fetch_cell = |(x, y): (usize, usize)| {
		let fetch_cell_base = base.clone();
		async move {
			let data = client
				.get(format!(
					"{}x{}-y{}-z{}",
					fetch_cell_base.clone(),
					x,
					y,
					zoom
				))
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
