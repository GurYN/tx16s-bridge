#!/usr/bin/env python3
"""PC side: receive the radio over UDP and feed it into a vJoy virtual joystick."""
import argparse
import ctypes
import os
import socket
import struct
import sys
import time

PACKET = struct.Struct("<4sII8h")  # magic, sequence, button bitmask, 8 axes
MAGIC = b"TX16"
VJOY_AXES = (0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37)  # X Y Z RX RY RZ SL0 SL1
VJOY_MAX = 0x8000
BUTTONS = 32
CENTER = [0] * len(VJOY_AXES)


def vjoy_value(axis):
    """Map -32767..32767 onto vJoy's 1..0x8000 range."""
    return (axis + 32767) * (VJOY_MAX - 1) // 65534 + 1


class VJoyDevice:
    def __init__(self, device):
        path = os.path.join(os.environ.get("ProgramFiles", r"C:\Program Files"),
                            "vJoy", "x64", "vJoyInterface.dll")
        try:
            self.dll = ctypes.WinDLL(path)
        except OSError:
            sys.exit(f"vJoy introuvable ({path}). Installe vJoy puis relance.")
        if not self.dll.vJoyEnabled():
            sys.exit("vJoy est installe mais desactive : active-le dans 'Configure vJoy'.")
        if not self.dll.AcquireVJD(device):
            sys.exit(f"Impossible de prendre le peripherique vJoy {device} : "
                     "il est deja utilise ou n'est pas configure.")
        self.device = device

    def set_axis(self, index, value):
        self.dll.SetAxis(value, self.device, VJOY_AXES[index])

    def set_button(self, index, pressed):
        self.dll.SetBtn(pressed, self.device, index + 1)

    def close(self):
        self.dll.RelinquishVJD(self.device)


class DryRunDevice:
    """Stands in for vJoy to test the network path on any OS."""

    def __init__(self):
        self.axes = [0] * len(VJOY_AXES)

    def set_axis(self, index, value):
        self.axes[index] = value

    def set_button(self, index, pressed):
        pass

    def close(self):
        pass


class Output:
    """Only writes values that changed, so vJoy is not hit 8 times per packet."""

    def __init__(self, device):
        self.device = device
        self.axes = [None] * len(VJOY_AXES)
        self.buttons = None

    def write(self, axes, buttons):
        for i, axis in enumerate(axes):
            value = vjoy_value(axis)
            if value != self.axes[i]:
                self.device.set_axis(i, value)
                self.axes[i] = value
        if buttons != self.buttons:
            changed = buttons ^ (self.buttons or 0) if self.buttons is not None else 0xFFFFFFFF
            for i in range(BUTTONS):
                if changed >> i & 1:
                    self.device.set_button(i, bool(buttons >> i & 1))
            self.buttons = buttons


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=47800)
    parser.add_argument("--device", type=int, default=1, help="numero du peripherique vJoy")
    parser.add_argument("--sender", help="n'accepte que cette adresse IP (celle du Mac)")
    parser.add_argument("--timeout", type=float, default=0.5,
                        help="secondes sans paquet avant de recentrer les axes")
    parser.add_argument("--dry-run", action="store_true", help="teste le reseau sans vJoy")
    args = parser.parse_args()

    device = DryRunDevice() if args.dry_run else VJoyDevice(args.device)
    output = Output(device)

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("0.0.0.0", args.port))
    sock.settimeout(0.1)
    print(f"En ecoute sur UDP {args.port}. Ctrl+C pour arreter.")

    connected = False
    last_sequence = 0
    last_packet = 0.0
    received = dropped = 0
    last_report = time.monotonic()
    try:
        while True:
            now = time.monotonic()
            try:
                data, source = sock.recvfrom(64)
            except socket.timeout:
                data = None
            except ConnectionResetError:  # Windows reports ICMP errors on UDP sockets
                data = None

            now = time.monotonic()
            valid = (data is not None and len(data) == PACKET.size
                     and (args.sender is None or source[0] == args.sender))
            if valid:
                magic, sequence, buttons, *axes = PACKET.unpack(data)
                valid = magic == MAGIC
            if valid and (not connected or sequence > last_sequence):
                if connected:
                    dropped += sequence - last_sequence - 1
                else:
                    print(f"Connecte a {source[0]}")
                    connected = True
                last_sequence = sequence
                last_packet = now
                received += 1
                output.write(axes, buttons)

            if connected and now - last_packet > args.timeout:
                print("Plus de signal : axes recentres et boutons relaches.")
                output.write(CENTER, 0)
                connected = False

            if now - last_report >= 5:
                if connected:
                    print(f"{received / (now - last_report):.0f} paquets/s, {dropped} perdus")
                received = dropped = 0
                last_report = now
    except KeyboardInterrupt:
        print()
    finally:
        output.write(CENTER, 0)
        device.close()


if __name__ == "__main__":
    main()
