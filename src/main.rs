use std::{borrow::Cow, sync::Arc, time::SystemTime};

#[tokio::main]
async fn main() {
	let app = clap::App::new(env!("CARGO_PKG_NAME"))
		.version(env!("CARGO_PKG_VERSION"))
		.author(env!("CARGO_PKG_AUTHORS"))
		.about(env!("CARGO_PKG_DESCRIPTION"))
		.arg(
			clap::Arg::with_name("URL")
				.required(true)
				.help("URL to the image base (temporary, will be URL to the image page)."),
		)
		.arg(
			clap::Arg::with_name("OUTPUT")
				.short("o")
				.long("output")
				.takes_value(true)
				.help("Output file name with .png or .jpg extension. Default: <Item ID>.png"),
		);
	let matches = app.get_matches();
	let url = matches.value_of("URL").unwrap();

	let client = Arc::new(reqwest::Client::new());

	let start = SystemTime::now();
	let page = deathrip::Page::try_fetch(&client, url).await.unwrap();
	deathrip::rip(client, &page.base_url, 8)
		.await
		.unwrap()
		.save(
			matches
				.value_of("OUTPUT")
				.map_or_else(
					|| Cow::Owned(format!("{}.png", page.title)),
					|out| Cow::Borrowed(out),
				)
				.as_ref(),
		)
		.unwrap();
	println!("Elapsed {}ms", start.elapsed().unwrap().as_millis());
}
