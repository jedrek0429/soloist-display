#!/usr/bin/env python3
"""Optional local PipeWire/PulseAudio monitor to WebSocket spectrum bridge."""
import asyncio
import json
import os
import shutil
import subprocess
import numpy as np
from websockets.asyncio.server import serve

HOST = os.environ.get("SOLOIST_DISPLAY_AUDIO_HOST", "127.0.0.1")
PORT = int(os.environ.get("SOLOIST_DISPLAY_AUDIO_PORT", "9091"))
SOURCE = os.environ.get("SOLOIST_DISPLAY_MONITOR", "")
RATE = 22050
CHUNK = 2048
clients = set()

async def stream(websocket):
    clients.add(websocket)
    try:
        await websocket.wait_closed()
    finally:
        clients.discard(websocket)

async def capture():
    if not shutil.which("parec"):
        raise SystemExit("Install pulseaudio-utils (parec), supported by pipewire-pulse.")
    args = ["parec", "--raw", "--format=s16le", "--rate", str(RATE), "--channels=1"]
    if SOURCE:
        args += ["--device", SOURCE]
    process = await asyncio.create_subprocess_exec(*args, stdout=asyncio.subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        while True:
            data = await process.stdout.readexactly(CHUNK * 2)
            samples = np.frombuffer(data, dtype="<i2").astype(np.float32) / 32768
            power = np.abs(np.fft.rfft(samples * np.hanning(CHUNK)))
            hz = np.fft.rfftfreq(CHUNK, 1 / RATE)
            edges = np.geomspace(40, 9000, 47)
            bars = []
            for a, b in zip(edges[:-1], edges[1:]):
                band = power[(hz >= a) & (hz < b)]
                value = float(np.max(band)) if band.size else 0
                bars.append(round(min(1, max(0, np.log1p(value) / 4.2)), 3))
            if clients:
                message = json.dumps({"type": "spectrum", "bars": bars})
                await asyncio.gather(*(client.send(message) for client in list(clients)), return_exceptions=True)
            await asyncio.sleep(0)
    finally:
        process.terminate()
        await process.wait()

async def main():
    async with serve(stream, HOST, PORT):
        await capture()

if __name__ == "__main__":
    asyncio.run(main())
