#!/usr/bin/env python3
import json
import os
import queue
import re
import struct
import subprocess
import sys
import threading
import time

BINARY = sys.argv[1]
OUT = sys.argv[2]
WIDTH = int(os.environ.get("CHECK_WIDTH", "6016"))
HEIGHT = int(os.environ.get("CHECK_HEIGHT", "3384"))
WINDOW = os.environ.get("CHECK_WINDOW", "2557 1440")
BACKING = os.environ.get("CHECK_BACKING", "2")
ROW = re.compile(r"^\s*([\d.]+) (B|KB|MB|GB)\s+([\d.]+) (B|KB|MB|GB)\s+([\d.]+) (B|KB|MB|GB)\s+\d+\s+(.+?)\s*$")
UNIT = {"B": 1 / 1048576, "KB": 1 / 1024, "MB": 1, "GB": 1024}
FRAME = re.compile(r"^0x[0-9a-f]+ \([^)]*\) ")
LIVE = re.compile(r"All zones: \d+ nodes \((\d+) bytes\)")
COLUMNS = ["iosurface", "owned physical footprint (unmapped)", "malloc large", "malloc small", "coreanimation"]


def screenshot(seed):
    block = WIDTH // 64
    palette = [bytes(((seed * 37 + i * 53) % 256, (i * 29) % 256, (seed * 11 + i * 7) % 256, 255)) for i in range(64)]
    bands = []
    for shift in range(8):
        row = b"".join(palette[(i + shift * 5) % 64] * block for i in range(64))
        bands.append(row + palette[0] * (WIDTH - len(row) // 4))
    noise = []
    for _ in range(4):
        row = bytearray(os.urandom(WIDTH * 4))
        row[3::4] = b"\xff" * WIDTH
        noise.append(bytes(row))
    rows = []
    for y in range(HEIGHT):
        band = (y // 120) % 10
        rows.append(noise[y % 4] if band == 9 else bands[band % 8])
    return b"".join(rows)


class Editor:
    def __init__(self, environment=None):
        self.process = subprocess.Popen([BINARY, "--editor"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=environment)
        self.replies = queue.Queue()
        self.received = 0
        self.step = "inicio"
        self.peaks = {}
        self.running = True
        threading.Thread(target=self.read, daemon=True).start()
        threading.Thread(target=self.sample, daemon=True).start()

    def exact(self, count):
        data = b""
        while len(data) < count:
            chunk = self.process.stdout.read(count - len(data))
            if not chunk:
                raise EOFError
            data += chunk
        return data

    def read(self):
        try:
            while True:
                header = json.loads(self.exact(struct.unpack("<I", self.exact(4))[0]))
                size = struct.unpack("<Q", self.exact(8))[0]
                while size > 0:
                    size -= len(self.exact(min(size, 1 << 20)))
                self.received += 1
                if "probed" in header:
                    self.replies.put(header["probed"]["result"])
        except (EOFError, ValueError, struct.error):
            self.replies.put(None)

    def send(self, header, payload=b""):
        encoded = json.dumps(header).encode()
        self.process.stdin.write(struct.pack("<I", len(encoded)) + encoded + struct.pack("<Q", len(payload)))
        self.process.stdin.write(payload)
        self.process.stdin.flush()

    def open(self, identifier):
        image = {"width": WIDTH, "height": HEIGHT, "bytesPerRow": WIDTH * 4, "colorSpaceName": "kCGColorSpaceDisplayP3"}
        created = time.time() - 978307200
        self.send({"open": {"id": identifier, "image": image, "scale": 2, "created": created}}, screenshot(identifier))

    def open_media(self, identifier, path, width, height):
        created = time.time() - 978307200
        media = {"id": identifier, "path": path, "gif": True, "width": width, "height": height, "created": created, "savedAutomatically": False}
        self.send({"openMedia": media})

    def probe(self, command):
        self.send({"probe": {"command": command}})
        try:
            result = self.replies.get(timeout=60)
        except queue.Empty:
            result = None
        if result is None:
            raise SystemExit(f"el editor no respondió a «{command}»")
        return result

    def footprint(self):
        text = subprocess.run(["footprint", str(self.process.pid)], capture_output=True, text=True).stdout
        rows = {}
        total = None
        for line in text.splitlines():
            match = ROW.match(line)
            if match:
                name = match.group(7)
                if name == "TOTAL":
                    total = float(match.group(1)) * UNIT[match.group(2)]
                else:
                    rows[name] = float(match.group(1)) * UNIT[match.group(2)]
        return total, rows

    def live(self):
        text = subprocess.run(["heap", str(self.process.pid)], capture_output=True, text=True).stdout
        match = LIVE.search(text)
        return int(match.group(1)) / 1048576 if match else 0

    def sample(self):
        while self.running:
            total, rows = self.footprint()
            if total is not None and total > self.peaks.get(self.step, (0, {}))[0]:
                self.peaks[self.step] = (total, rows)
            time.sleep(0.3)

    def close(self):
        self.running = False
        self.process.stdin.close()
        try:
            self.process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.process.kill()


def tool(arguments, limit, tail=0):
    text = subprocess.run(arguments, capture_output=True, text=True).stdout
    kept = []
    for row in [row for row in text.splitlines() if row.strip()][:limit]:
        frames = row.split(" | ")
        if tail and len(frames) > 2:
            names = [FRAME.sub("", frame) for frame in frames[1:]]
            row = frames[0].split(": ")[0] + ": " + " | ".join(names[-tail:])
        kept.append(row[:2600])
    return "\n".join(kept)


def dump(name, pid):
    sections = [
        ("footprint", tool(["footprint", str(pid)], 60)),
        ("heap", tool(["heap", str(pid), "-s"], 190)),
        ("stacks", tool(["malloc_history", str(pid), "-allBySize"], 90, tail=40)),
    ]
    text = "\n".join(f"--- {title} {name}\n{body}" for title, body in sections)
    print(text, flush=True)
    with open(os.path.join(OUT, f"perfil-{name}.txt"), "w") as file:
        file.write(text + "\n")


def profile():
    logged = dict(os.environ, MallocStackLogging="lite")
    editor = Editor(logged)
    editor.open(1)
    time.sleep(4)
    editor.probe(f"resize {WINDOW}")
    editor.probe(f"scale {BACKING}")
    time.sleep(8)
    dump("editor-1", editor.process.pid)
    editor.open(2)
    time.sleep(3)
    editor.open(3)
    time.sleep(8)
    for shape in ("rectangle", "arrow"):
        editor.probe(f"draw {shape}")
    time.sleep(8)
    dump("editor-3", editor.process.pid)
    editor.probe("zoom 1")
    time.sleep(8)
    dump("editor-zoom", editor.process.pid)
    editor.close()
    bar = subprocess.Popen([BINARY], env=logged)
    time.sleep(12)
    dump("barra", bar.pid)
    bar.terminate()
    try:
        bar.wait(timeout=10)
    except subprocess.TimeoutExpired:
        bar.kill()


def columns(rows):
    values = []
    for name in COLUMNS:
        values.append(sum(value for key, value in rows.items() if key.lower().replace("_", " ").startswith(name)))
    listed = sum(values)
    return values + [sum(rows.values()) - listed]


def line(label, total, rows):
    cells = "".join(f"{value:9.0f}" for value in columns(rows))
    return f"{label:<34}{total:8.0f}{cells}"


def main():
    os.makedirs(OUT, exist_ok=True)
    editor = Editor()
    table = []

    def step(name):
        editor.step = name

    def measure(settle):
        time.sleep(settle)
        total, rows = editor.footprint()
        if total is None:
            raise SystemExit("footprint no devolvió datos")
        table.append((editor.step, total, rows))
        print(f"{editor.step}: {total:.0f} MB | {editor.live():.1f} MB en uso | {editor.probe('state')}", flush=True)

    def frame(name):
        print(f"cuadro {name}: {editor.probe('frame ' + os.path.join(OUT, name))}", flush=True)
        subprocess.run(["screencapture", "-x", os.path.join(OUT, "pantalla-" + name)])

    def expect(*fragments):
        state = editor.probe("state")
        missing = [fragment for fragment in fragments if fragment not in state]
        if missing:
            raise SystemExit(f"{editor.step}: falta {missing} en «{state}»")

    step("1 pestaña")
    editor.open(1)
    time.sleep(4)
    editor.probe(f"resize {WINDOW}")
    editor.probe(f"scale {BACKING}")
    measure(8)
    expect("tabs=1", "bar=false", " mapped ")
    frame("1-una.png")

    step("3 pestañas")
    editor.open(2)
    time.sleep(3)
    editor.open(3)
    measure(8)
    expect("tabs=3", "current=2", "bar=true")

    step("4 pestañas")
    editor.open(4)
    measure(8)
    expect("tabs=4", "current=3")

    step("tras cambiar de pestaña")
    for index in (0, 1, 3):
        editor.probe(f"select {index}")
        expect(f"current={index}")
        time.sleep(2)
    measure(8)

    step("tras clic en pestaña")
    for index in (1, 2, 3):
        editor.probe(f"click {index}")
        expect(f"current={index}")
        time.sleep(2)
    measure(8)
    frame("2-pestanas.png")

    step("tras 20 cambios más")
    for turn in range(20):
        editor.probe(f"select {turn % 4}")
    measure(8)

    step("tras 60 cambios más")
    for turn in range(60):
        editor.probe(f"select {turn % 4}")
    measure(8)

    step("dibujando")
    for tool in ("rectangle", "arrow", "ellipse"):
        editor.probe(f"draw {tool}")
    measure(1)
    frame("3-dibujo.png")

    step("dibujo en reposo")
    measure(10)
    expect("shapes=3")
    editor.probe("select 0")
    editor.probe("select 3")
    editor.probe("undo")
    expect("current=3", "shapes=2")

    step("con una imagen fijada")
    editor.probe("pin")
    measure(8)
    expect("windows=2")
    frame("4-fijada.png")
    editor.probe("unpin")
    expect("windows=1")

    step("zoom al 100 %")
    editor.probe("zoom 1")
    measure(6)
    frame("5-zoom.png")
    editor.probe("select 0")
    editor.probe("select 3")
    expect("current=3", "zoom=1.0")

    step("minimizada")
    editor.probe("minimize")
    measure(8)
    expect("shown=false", "chrome=false", "frame=0x0")

    step("1 pestaña tras cerrar 3")
    editor.probe("restore")
    time.sleep(3)
    editor.probe("close 0")
    time.sleep(2)
    editor.probe("shut")
    time.sleep(2)
    editor.probe("close 0")
    measure(10)
    expect("tabs=1", "bar=false", "shown=true", " mapped ")
    frame("6-final.png")

    step("con una grabación abierta")
    recording = os.path.join(OUT, "grabacion.gif")
    subprocess.run(["sips", "-s", "format", "gif", os.path.join(OUT, "pantalla-1-una.png"), "--out", recording], capture_output=True)
    editor.open_media(5, recording, 512, 384)
    measure(8)
    expect("tabs=2", "current=1", "media=gif")
    subprocess.run(["screencapture", "-x", os.path.join(OUT, "pantalla-7-grabacion.png")])
    editor.probe("shut")
    expect("tabs=1", "current=0", " mapped ")

    environment = subprocess.run(["ps", "-E", "-p", str(editor.process.pid)], capture_output=True, text=True).stdout
    if "MallocSpaceEfficient=1" not in environment:
        raise SystemExit("el editor no corre con MallocSpaceEfficient")
    editor.close()
    header = f"{'paso':<34}{'total':>8}{'IOSurf':>9}{'Propia':>9}{'MallocL':>9}{'MallocS':>9}{'CoreAnim':>9}{'resto':>9}"
    lines = ["Memoria del editor en MB, en reposo", header]
    lines += [line(name, total, rows) for name, total, rows in table]
    lines += [fallback()]
    lines += ["", "Pico de cada paso", header]
    lines += [line(name, *editor.peaks[name]) for name, _, _ in table if name in editor.peaks]
    lines += ["", f"Barra de menú en reposo: {bar():.0f} MB"]
    report = "\n".join(lines)
    print(report)
    with open(os.path.join(OUT, "memoria.txt"), "w") as file:
        file.write(report + "\n")


def fallback():
    editor = Editor()
    editor.step = "si Core Animation copia el cuadro"
    editor.open(1)
    time.sleep(4)
    editor.probe(f"resize {WINDOW}")
    editor.probe(f"scale {BACKING}")
    time.sleep(3)
    before = editor.probe("state")
    editor.probe("untag")
    time.sleep(6)
    total, rows = editor.footprint()
    state = editor.probe("state")
    editor.close()
    if " mapped " not in before or " surface " not in state:
        raise SystemExit(f"{editor.step}: «{before}» y después «{state}»")
    return line(editor.step, total, rows)


def bar():
    bundle = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(BINARY))))
    started = time.time()
    subprocess.run(["open", bundle], check=True)
    time.sleep(12)
    rows = subprocess.run(["ps", "-axo", "pid=,args="], capture_output=True, text=True).stdout.splitlines()
    pids = [row.split()[0] for row in rows if row.strip().endswith("/Contents/MacOS/ShareK")]
    if len(pids) != 1:
        raise SystemExit(f"la barra de menú no quedó corriendo una sola vez: {pids}")
    pid = pids[0]
    text = subprocess.run(["footprint", pid], capture_output=True, text=True).stdout
    environment = subprocess.run(["ps", "-E", "-p", pid], capture_output=True, text=True).stdout
    since = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(started))
    predicate = f'processID == {pid} AND subsystem == "com.apple.AppKit" AND category == "StatusBar"'
    status = subprocess.run(["log", "show", "--start", since, "--predicate", predicate, "--style", "compact"], capture_output=True, text=True).stdout
    subprocess.run(["screencapture", "-x", "-R0,0,1024,40", os.path.join(OUT, "barra.png")], capture_output=True)
    subprocess.run(["kill", pid])
    if "MallocSpaceEfficient=1" not in environment:
        raise SystemExit("la barra de menú no corre con MallocSpaceEfficient")
    failures = [row for row in status.splitlines() if "failed" in row]
    if failures:
        raise SystemExit("el ícono de la barra de menú no se mostró: " + failures[0])
    for row in text.splitlines():
        match = ROW.match(row)
        if match and match.group(7) == "TOTAL":
            return float(match.group(1)) * UNIT[match.group(2)]
    raise SystemExit("footprint no devolvió datos de la barra de menú")

main()
if os.environ.get("CHECK_PROFILE") == "true":
    profile()
