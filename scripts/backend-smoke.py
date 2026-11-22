#!/usr/bin/env python3
"""Exercise the same mpv JSON IPC contract, without a desktop or audio hardware."""
import json
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path


def main():
    for program in ("mpv", "ffmpeg"):
        if not shutil.which(program):
            raise SystemExit(f"{program} is required; run inside nix develop")
    with tempfile.TemporaryDirectory(prefix="cintec-smoke-") as tmp:
        root = Path(tmp)
        files = []
        for extension, codec in (("flac", "flac"), ("opus", "libopus"), ("mp3", "libmp3lame"), ("m4a", "aac")):
            path = root / f"test.{extension}"
            subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", codec, "-metadata", "title=Cintec smoke test", str(path)], check=True)
            files.append(path)
        saved_position = 0.0
        # A fresh process for each pass exercises the reconnect strategy.
        for attempt in range(2):
            ipc = root / f"mpv-{attempt}.sock"
            process = subprocess.Popen(["mpv", "--idle=yes", "--no-video", "--no-config", "--load-scripts=no", "--input-terminal=no", "--audio-display=no", "--terminal=no", "--ao=null", f"--input-ipc-server={ipc}"], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            try:
                with socket.socket(socket.AF_UNIX) as client:
                    deadline = time.monotonic() + 5
                    while True:
                        try:
                            client.connect(str(ipc))
                            break
                        except (FileNotFoundError, ConnectionRefusedError):
                            if time.monotonic() > deadline or process.poll() is not None:
                                raise RuntimeError("mpv IPC did not start: " + process.stderr.read().decode())
                            time.sleep(.02)
                    client.settimeout(5)
                    stream = client.makefile("r")

                    def send(command):
                        client.sendall((json.dumps({"command": command}) + "\n").encode())

                    def wait_for(predicate):
                        deadline = time.monotonic() + 6
                        while time.monotonic() < deadline:
                            event = json.loads(stream.readline())
                            if event.get("error") not in (None, "success", "property unavailable"):
                                raise AssertionError(event)
                            if predicate(event):
                                return event
                        raise TimeoutError("Missing mpv event")

                    for n, prop in enumerate(("time-pos", "duration", "pause", "idle-active", "metadata", "audio-params", "audio-bitrate", "volume", "path")):
                        send(["observe_property", n, prop])
                    for path in files if attempt == 0 else files[:1]:
                        send(["loadfile", str(path), "replace"])
                        send(["set_property", "pause", True])
                        wait_for(lambda e: e.get("event") == "file-loaded")
                        send(["get_property", "audio-params"])
                        params = wait_for(lambda e: isinstance(e.get("data"), dict) and "samplerate" in e["data"])
                        assert params["data"]["samplerate"] > 0
                        send(["seek", saved_position if attempt else .5, "absolute+exact"])
                        send(["get_property", "time-pos"])
                        position = wait_for(lambda e: isinstance(e.get("data"), (int, float)) and e["data"] >= .4)
                        saved_position = position["data"]
                        send(["set_property", "pause", False])
                        wait_for(lambda e: e.get("event") == "end-file" and e.get("reason") == "eof")
                        print(f"PASS {path.suffix}: metadata, seek, play, EOF" + (" (reconnected)" if attempt else ""))
            finally:
                process.terminate()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    main()
