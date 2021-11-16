use reqwest::Client;

pub async fn determine_max_zoom(client: &Client, mut base: String) -> Result<u32, reqwest::Error> {
	let mut level = 0;
	base.reserve(10);
	base.push_str("x0-y0-z1");
	let z_index = base.len() - 1;

	loop {
		dbg!(&base);
		let response = client.head(&base).send().await?;
		if response.status().is_success() {
			level += 1;
			let next_level = level + 1;
			let next_level_str = next_level.to_string();
			base.replace_range(z_index.., &next_level_str);
		} else {
			break;
		}
	}

	Ok(level)
}
