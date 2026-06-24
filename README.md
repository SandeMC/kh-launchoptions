# Kingdom Hearts: Steam Launch Options setuP utility (KH:SLOP)

Nasty and Clean Steam launch options setup utility for tailored Steam-native launch options for installed KH games, either launching them directly or passing them through my custom launchers, for Windows and Linux.

<img width="277" height="251" alt="image" src="https://github.com/user-attachments/assets/5f27afcd-e326-44c6-aba8-8c89edd65387" />

This app was made to be used alongside [kh-downloader](https://github.com/SandeMC/kh-downloader) and [Kingdom-Hearts-Launchers](https://github.com/SandeMC/Kingdom-Hearts-Launchers), but can be used on it's own.

## Usage

Just download the latest [release](https://github.com/SandeMC/kh-launchoptions/releases/latest) for your platform, fully close Steam, open the terminal and use one of the following modes:

- `kh-launchoptions --direct` (detects installed game EXEs and sets up the launch options for those)
- `kh-launchoptions --custom-launcher` (passes the arguments down to my [custom launcher](https://github.com/SandeMC/Kingdom-Hearts-Launchers))

Report any issue at [Issues](https://github.com/SandeMC/kh-downloader/issues). 2.8 support coming later.

## How

Steam stores launch option metadata in a binary file at Steam\appcache\appinfo.vdf. This tool: locates your Steam installation via the Windows registry, then reads steamapps\libraryfolders.vdf to find all Steam library paths across all drives, searches every library for appmanifest_2552430.acf and reads the installdir field to get the exact game folder.

After that, it parses appinfo.vdf, finds the record of the game, replaces its launch section with the new slots, recomputes the checksums, and writes the file back. This part of the code is derived from [tralph3/Steam-Metadata-Editor](https://github.com/tralph3/Steam-Metadata-Editor) - most credit goes to them.

I should probably also disclose that this app's development was AI-assisted. I didn't know Rust before this so it kind of aided me in learning some Rust things.

## Building

Requires Rust (stable). No other toolchain dependencies.

```
cargo build --release
```

The binary will be at target\release\. It has no console window and shows a native Windows message box on completion or error.
