use reqwest::Client;

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
	determine_limit(client, base, &params).await
}

pub async fn determine_rows(
	client: &Client,
	base: String,
	zoom: usize,
) -> Result<usize, reqwest::Error> {
	let params = format!("z{}-x0-y1", zoom);
	determine_limit(client, base, &params).await
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
