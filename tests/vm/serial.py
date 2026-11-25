#!/usr/bin/env python3
"""Talks to a virtual machine's serial console over a UNIX socket.

usage: serial.py SOCKET LOG < SCRIPT

Script lines:
    expect [SECONDS] REGEX     wait for the pattern in new output
    screen [SECONDS] REGEX     wait for the pattern on the emulated 80x24 screen
    send TEXT                  type TEXT and press Enter
    key TEXT                   type TEXT with escapes such as \\x1b[6~
    refuse REGEX               fail if the pattern ever appears
    monitor COMMAND            run a QEMU monitor command
    sleep SECONDS
"""

import codecs
import re
import select
import socket
import sys
import time

CONTROL = re.compile(r"\x1b\[([0-9;?]*)([A-Za-z])|\x1b[()][0-9A-B]|\x1b[=>78]")


class Screen:
    """Enough of a terminal to read what a full-screen program shows."""

    def __init__(self, columns=80, rows=24):
        self.columns, self.rows = columns, rows
        self.pending = ""
        self.row = self.column = 0
        self.clear()

    def clear(self):
        self.cells = [[" "] * self.columns for _ in range(self.rows)]

    def feed(self, text):
        text = self.pending + text
        self.pending = ""
        position = 0
        while position < len(text):
            character = text[position]
            if character == "\x1b":
                match = CONTROL.match(text, position)
                if match:
                    if match.group(2):
                        self.control(match.group(1), match.group(2))
                    position = match.end()
                elif len(text) - position < 16:
                    # Incomplete; the rest comes with the next read.
                    self.pending = text[position:]
                    return
                else:
                    position += 1
                continue
            position += 1
            if character == "\r":
                self.column = 0
            elif character == "\n":
                self.line_feed()
            elif character == "\b":
                self.column = max(0, self.column - 1)
            elif character >= " ":
                if self.column >= self.columns:
                    self.column = 0
                    self.line_feed()
                self.cells[self.row][self.column] = character
                self.column += 1

    def line_feed(self):
        if self.row + 1 < self.rows:
            self.row += 1
        else:
            self.cells.pop(0)
            self.cells.append([" "] * self.columns)

    def control(self, parameters, command):
        numbers = [int(n) if n.isdigit() else 0 for n in parameters.lstrip("?").split(";")]
        if command == "H":
            row = numbers[0] or 1
            column = (numbers[1] if len(numbers) > 1 else 0) or 1
            self.row = min(row, self.rows) - 1
            self.column = min(column, self.columns) - 1
        elif command == "J" and numbers[0] == 2:
            self.clear()
        elif command == "K":
            for column in range(self.column, self.columns):
                self.cells[self.row][column] = " "

    def text(self):
        return "\n".join("".join(row).rstrip() for row in self.cells)


class Console:
    def __init__(self, path, log):
        deadline = time.time() + 30
        while True:
            try:
                self.socket = socket.socket(socket.AF_UNIX)
                self.socket.connect(path)
                break
            except OSError:
                if time.time() > deadline:
                    raise
                time.sleep(0.5)
        self.log = open(log, "wb")
        self.buffer = ""
        self.refused = []
        self.screen = Screen()
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def read(self, seconds):
        ready, _, _ = select.select([self.socket], [], [], seconds)
        if not ready:
            return
        data = self.socket.recv(65536)
        if not data:
            raise EOFError("the machine closed its console")
        self.log.write(data)
        self.log.flush()
        text = self.decoder.decode(data)
        self.buffer += text
        self.screen.feed(text)

    def expect(self, pattern, seconds, on_screen=False):
        deadline = time.time() + seconds
        while True:
            for refused in self.refused:
                if re.search(refused, self.buffer):
                    raise AssertionError(f"refused output appeared: {refused}")
            match = re.search(pattern, self.screen.text() if on_screen else self.buffer)
            if match:
                self.buffer = "" if on_screen else self.buffer[match.end():]
                return match
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError(
                    f"no {pattern!r} in:\n{self.buffer[-600:]!r}\nscreen:\n{self.screen.text()}"
                )
            self.read(min(remaining, 1))

    def send(self, text):
        # Slowly: console input has no flow control.
        for character in text:
            self.socket.sendall(character.encode())
            time.sleep(0.01)

    def monitor(self, serial, command):
        """Runs a QEMU monitor command; its socket is beside the serial one."""
        with socket.socket(socket.AF_UNIX) as monitor:
            monitor.connect(serial.removesuffix("serial") + "monitor")
            time.sleep(0.1)
            monitor.sendall(command.encode() + b"\n")
            time.sleep(0.3)

    def key(self, text):
        # At once: a lone escape character would be taken for the Esc key.
        self.socket.sendall(text.encode().decode("unicode_escape").encode())
        time.sleep(0.2)


def main():
    console = Console(sys.argv[1], sys.argv[2])
    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line.strip() or line.startswith("#"):
            continue
        command, _, rest = line.partition(" ")
        if command in ("expect", "screen"):
            match = re.match(r"(\d+) (.*)", rest)
            seconds, pattern = (int(match[1]), match[2]) if match else (60, rest)
            found = console.expect(pattern, seconds, command == "screen")
            print("ok:", " ".join(found.group(0).split())[:100], flush=True)
        elif command == "send":
            console.send(rest + "\r")
        elif command == "key":
            console.key(rest)
        elif command == "refuse":
            console.refused.append(rest)
        elif command == "monitor":
            console.monitor(sys.argv[1], rest)
        elif command == "sleep":
            deadline = time.time() + float(rest)
            while time.time() < deadline:
                console.read(0.2)
        else:
            raise SystemExit(f"unknown command {command!r}")


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, TimeoutError, EOFError) as error:
        print("FAIL:", error, file=sys.stderr)
        sys.exit(1)
