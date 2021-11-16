#[tokio::main]
async fn main() {
	const DEFAULT_OUTPUT_FILE_NAME: &str = "dss_rip.png";
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
				.help("Output file name.")
				.default_value(DEFAULT_OUTPUT_FILE_NAME),
		);
	let matches = app.get_matches();
	let url = matches.value_of("URL").unwrap();
	dbg!(url);

	let client = reqwest::Client::new();

	deathrip::rip(&client, url.to_string())
		.await
		.unwrap()
		.save(
			matches
				.value_of("OUTPUT")
				.unwrap_or(DEFAULT_OUTPUT_FILE_NAME),
		)
		.unwrap();
}
