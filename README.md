# NetEase Cloud Music Downloader

A simple command-line tool to download songs, albums, and lyrics from NetEase Cloud Music.

This project is a Rust port of the [Gaohaoyang's project](https://github.com/Gaohaoyang/netease-music-downloader).

## What This Program Does

This tool lets you:
- download one or more songs by ID
- download all songs from an album
- download lyrics only (`.lrc` files)
- use a manual proxy or automatic proxy fallback when direct access fails
- resolve DJ program IDs to the underlying song with `--program` / `-P`

Downloaded files are saved in a local `downloads/` folder.

## Installation

### Option 1: Build from source (recommended)

```bash
git clone <your-repo-url>
cd netease-music-downloader
cargo build --release
```

Run with:

```bash
./target/release/netease_music_downloader_rs --help
```

### Option 2: Run without building release

```bash
cargo run -- --help
```

## Quick Usage

### Download one song

```bash
cargo run -- download 426832090
```

### Download multiple songs

```bash
cargo run -- download 426832090 1381755293
```

### Download song IDs from file

```bash
cargo run -- download -f ids.txt
```

Example `ids.txt`:

```text
426832090
1381755293
# comments are ignored
```

### Download a full album

```bash
cargo run -- album 34836039
```

### Download lyrics only

```bash
cargo run -- lyrics 426832090
```

### Download album lyrics only

```bash
cargo run -- album-lyrics 34836039
```

## Program Mode (`--program` / `-P`)

Use this when your IDs are DJ program IDs (not direct song IDs).

```bash
cargo run -- download -P 3074011314
cargo run -- lyrics --program 3074011314
```

## Proxy Options

### Manual proxy

```bash
cargo run -- --proxy http://127.0.0.1:7890 download 426832090
```

### Automatic proxy fallback

```bash
cargo run -- --auto-proxy download 426832090
```

## Output Structure

```text
downloads/
├── Artist-Song.mp3
├── Artist-Song.lrc
└── Artist-Album/
    ├── 01.Artist-Track.mp3
    ├── 01.Artist-Track.lrc
    └── ...
```

## Disclaimer

This project is provided for educational and research purposes only.

The author does not encourage or support piracy, copyright infringement, or any illegal distribution of protected content. You are responsible for complying with the laws and terms of service applicable in your country and platform.

## License
BSD 2-clause