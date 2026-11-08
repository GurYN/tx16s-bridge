#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["hidapi>=0.14"]
# ///
"""Mac side: read the TX16S over USB HID and stream its axes and buttons over UDP."""
import argparse
import math
import socket
import struct
import sys
import time

PACKET = struct.Struct("<4sII8h")  # magic, sequence, button bitmask, 8 axes
MAGIC = b"TX16"
AXES = 8

# EdgeTX joystick report, from the radio's HID descriptor:
# 24 buttons (3 bytes), 8 axes (uint16, 0..2048), battery (1 byte). No report ID.
REPORT = struct.Struct("<3s8HB")
AXIS_CENTER = 1024
HID_GENERIC_DESKTOP = 1
HID_JOYSTICK_USAGES = (4, 5, 8)  # joystick, gamepad, multi-axis controller
NAME_HINTS = ("tx16", "radiomaster", "edgetx", "opentx")


def open_radio(hid, index):
    devices = [d for d in hid.enumerate()
               if d["usage_page"] == HID_GENERIC_DESKTOP and d["usage"] in HID_JOYSTICK_USAGES]
    if not devices:
        sys.exit("Aucun joystick detecte. Branche la radio et choisis 'USB Joystick' sur son ecran.")
    print("Joysticks detectes :")
    for i, device in enumerate(devices):
        print(f"  [{i}] {device['product_string']}")
    if index is not None:
        chosen = devices[index]
    else:
        matches = [d for d in devices
                   if any(h in (d["product_string"] or "").lower() for h in NAME_HINTS)]
        if len(matches) == 1:
            chosen = matches[0]
        elif len(devices) == 1:
            chosen = devices[0]
        else:
            sys.exit("Plusieurs joysticks possibles : relance avec --joystick N.")
    radio = hid.device()
    radio.open_path(chosen["path"])
    print(f"Radio : {chosen['product_string']}")
    return radio


def decode_report(report):
    if len(report) != REPORT.size:
        sys.exit(f"Rapport HID de {len(report)} octets au lieu de {REPORT.size} : "
                 "le mode joystick de la radio n'est pas le mode classique.")
    buttons, *axes, _battery = REPORT.unpack(bytes(report))
    scaled = [max(-32767, min(32767, round((a - AXIS_CENTER) * 32767 / AXIS_CENTER))) for a in axes]
    return scaled, int.from_bytes(buttons, "little")


def test_pattern(started):
    """Slow sine waves, to check the whole chain without the radio."""
    elapsed = time.monotonic() - started
    axes = [round(math.sin(elapsed + i * 0.8) * 32767) for i in range(AXES)]
    return axes, 1 << (int(elapsed) % 24)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host", help="adresse IP du PC")
    parser.add_argument("--port", type=int, default=47800)
    parser.add_argument("--joystick", type=int, help="index du joystick, si plusieurs")
    parser.add_argument("--test", action="store_true", help="envoie un signal de test, sans radio")
    parser.add_argument("--show", action="store_true", help="affiche les valeurs lues")
    args = parser.parse_args()

    radio = None
    if not args.test:
        import hid

        radio = open_radio(hid, args.joystick)

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    target = (args.host, args.port)
    print(f"Envoi vers {target[0]}:{target[1]}. Ctrl+C pour arreter.")

    KEEPALIVE_MS = 20  # resend the last state if the radio stays quiet
    started = last_show = time.monotonic()
    sequence = 0
    state = None
    try:
        while True:
            if args.test:
                state = test_pattern(started)
                time.sleep(KEEPALIVE_MS / 1000)
            else:
                try:
                    report = radio.read(64, KEEPALIVE_MS)
                except (OSError, ValueError):
                    sys.exit("Radio debranchee.")
                if report:
                    state = decode_report(report)
            if state is None:
                continue
            axes, buttons = state
            sequence += 1
            sock.sendto(PACKET.pack(MAGIC, sequence, buttons, *axes), target)
            now = time.monotonic()
            if args.show and now - last_show >= 0.1:
                last_show = now
                print(" ".join(f"{a:+6d}" for a in axes), f"btn={buttons:024b}", end="\r")
    except KeyboardInterrupt:
        print()


if __name__ == "__main__":
    main()
