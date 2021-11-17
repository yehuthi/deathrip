# deathrip

A command-line tool to download full-resolution images from [The Dead Sea Scrolls Digital Library](https://www.deadseascrolls.org.il/).

Accept the library's [terms of service](https://www.deadseascrolls.org.il/terms) before usage.

# Installation

You can install with `cargo`:
```bash
cargo install deathrip
```

Or on Windows download the latest [release](https://github.com/yehuthi/deathrip/releases).

# Usage
Simple usage:
```ps1
deathrip <page URL | item ID> [-o <destination>.<"png"|"jpg">]
```

E.g.
```bash
# Equivalent:
deathrip https://www.deadseascrolls.org.il/explore-the-archive/image/B-314643
deathrip B-314643

# With destination:
deathrip B-314643 -o ten_commandments.jpg
```

For more usage information, run with `--help`.
