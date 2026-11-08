#!/bin/sh
# Builds dist/tx16s-bridge-windows.zip: the receiver's sources and a trimmed
# workspace, so the PC only downloads what the receiver needs.
set -eu
cd "$(dirname "$0")/.."

name=tx16s-bridge-windows
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
root="$stage/$name"
mkdir -p "$root/crates" "$root/python"

cp -R crates/protocol crates/receiver "$root/crates/"
cp Cargo.lock LICENSE "$root/"
cp python/receiver.py "$root/python/"
find "$root" -name .DS_Store -delete

cat > "$root/Cargo.toml" <<'TOML'
[workspace]
resolver = "3"
members = ["crates/protocol", "crates/receiver"]

[workspace.package]
edition = "2024"
version = "0.1.0"
license = "MIT"
TOML

cat > "$root/README.txt" <<'TEXT'
TX16S Bridge - Windows receiver
===============================

Receives the radio sent by the "TX16S Bridge" Mac app and exposes it to games
as a vJoy joystick. Full documentation: README.md in the project repository.

1. Install vJoy, then in "Configure vJoy": device 1, 8 axes, 32 buttons.
2. Install Rust from https://rustup.rs (accept the Visual Studio Build Tools
   it offers).
3. In this folder, build:
       cargo build --release
4. From an ADMINISTRATOR terminal, install and start the service:
       target\release\tx16s-receiver.exe install
   The Windows network must be set to "Private" (firewall rule).

   To update an existing install, run "tx16s-receiver uninstall" first.

Other commands: status, stop, start, uninstall
Run without the service: target\release\tx16s-receiver.exe run
Configuration:           C:\ProgramData\tx16s-bridge\config.toml
Logs:                    C:\ProgramData\tx16s-bridge\logs\

Fallback: python\receiver.py is the original receiver, with no service and no
dependency beyond Python and vJoy: python python\receiver.py
TEXT

mkdir -p dist
rm -f "dist/$name.zip"
(cd "$stage" && zip -qr -X "$OLDPWD/dist/$name.zip" "$name")
echo "Built dist/$name.zip"
