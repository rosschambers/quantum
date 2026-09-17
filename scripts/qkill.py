#!/usr/bin/env python3
"""qkill — quantum-aware click-to-kill for Hyprland.

Pick a point with ``slurp -p`` and resolve what lives under it:

* A window owned by the quantum daemon gets a GRACEFUL CLOSE
  (``hyprctl dispatch closewindow``). Quantum draws every one of its
  windows from a single process, so a raw SIGKILL on any of them takes
  the whole shell down; a close request closes just that window.
* Everything else is force-killed (SIGKILL to the owning pid), matching
  plain ``hyprctl kill`` semantics.
* Quantum LAYER surfaces (bar, clock, timers) cannot receive a close
  request (layer-shell has no close protocol), so those keep kill
  semantics too.

Resolution is geometric: the focused window wins when the point falls
inside it, otherwise the first client whose rectangle contains the
point, then quantum layer surfaces. Overlapping windows are a
heuristic — the compositor knows the exact stacking order, slurp only
reports coordinates. This script exists to make "click the quantum
window" safe; for ambiguous overlaps prefer the right-click menu, which
closes by exact address.

Test hooks: ``--point X Y`` skips slurp, ``--dry-run`` prints the
decision instead of acting.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys

QUANTUM_NAMESPACE_PREFIX = "quantum-widget-"
QUANTUM_CLASS_HINTS = ("quantum",)


def hyprctl(*args: str) -> str:
    return subprocess.run(
        ["hyprctl", *args],
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def pick_point() -> tuple[int, int] | None:
    if shutil.which("slurp") is None:
        sys.exit("qkill: slurp not found on PATH")
    result = subprocess.run(["slurp", "-p"], capture_output=True, text=True)
    if result.returncode != 0:
        return None  # user cancelled (Escape)
    x_text, y_text = result.stdout.strip().split(",")
    return int(x_text), int(y_text)


def contains(rect: tuple[int, int, int, int], point: tuple[int, int]) -> bool:
    x, y, width, height = rect
    return x <= point[0] < x + width and y <= point[1] < y + height


def parse_clients() -> list[dict]:
    return json.loads(hyprctl("clients", "-j"))


LAYER_PATTERN = re.compile(
    r"Layer\s+(?P<address>0x[0-9a-f]+):\s+xywh:\s+"
    r"(?P<x>-?\d+)\s+(?P<y>-?\d+)\s+(?P<w>\d+)\s+(?P<h>\d+).*?"
    r"namespace:\s+(?P<namespace>\S+).*?pid:\s+(?P<pid>\d+)"
)


def parse_layers() -> list[dict]:
    return [match.groupdict() for match in LAYER_PATTERN.finditer(hyprctl("layers"))]


def is_quantum_client(client: dict, daemon_pids: set[int]) -> bool:
    if client.get("pid") in daemon_pids:
        return True
    window_class = (client.get("class") or "").lower()
    return any(hint in window_class for hint in QUANTUM_CLASS_HINTS)


def rect_of(client: dict) -> tuple[int, int, int, int] | None:
    at, size = client.get("at"), client.get("size")
    if not at or not size:
        return None
    return int(at[0]), int(at[1]), int(size[0]), int(size[1])


def resolve(point: tuple[int, int]) -> tuple[str, dict] | None:
    """Return (kind, target) for the surface under the point.

    kind is "client" or "layer". Smallest containing rect wins — a small
    window on top of a big one beats the big one — with the focused client as
    the tiebreak between equal-area clients. Layer-surface rectangles lie
    (they span the whole output; input regions clip what actually receives
    clicks), so they are consulted only when no client contains the point.

    Known limitation: a window overlapping a background-layer widget (the
    clock) is visually on top but loses here to the smaller surface rect.
    """
    clients = [c for c in parse_clients() if rect_of(c) and contains(rect_of(c), point)]
    if clients:
        clients.sort(key=lambda c: (area(rect_of(c)), 0 if c.get("focused") else 1))
        return ("client", clients[0])

    layers = [
        layer
        for layer in parse_layers()
        if contains((int(layer["x"]), int(layer["y"]), int(layer["w"]), int(layer["h"])), point)
    ]
    if layers:
        layers.sort(key=lambda layer: int(layer["w"]) * int(layer["h"]))
        return ("layer", layers[0])
    return None


def area(rect: tuple[int, int, int, int]) -> int:
    return rect[2] * rect[3]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--point", nargs=2, type=int, metavar=("X", "Y"), help="skip slurp")
    parser.add_argument("--dry-run", action="store_true", help="print the decision, do not act")
    args = parser.parse_args()

    point = tuple(args.point) if args.point else pick_point()
    if point is None:
        return 0  # cancelled

    daemon_pids = {
        int(layer["pid"])
        for layer in parse_layers()
        if layer["namespace"].startswith(QUANTUM_NAMESPACE_PREFIX)
    }

    resolved = resolve(point)
    if resolved is None:
        return 0  # nothing under the point
    kind, target = resolved

    if kind == "client" and is_quantum_client(target, daemon_pids):
        action = ["hyprctl", "dispatch", "closewindow", f"address:{target['address']}"]
        what = f"quantum window {target.get('class') or target.get('title', '')}"
    elif kind == "client":
        action = ["kill", "-KILL", str(target["pid"])]
        what = f"{target.get('class') or 'window'} pid {target['pid']}"
    elif kind == "layer" and target["namespace"].startswith(QUANTUM_NAMESPACE_PREFIX):
        # Quantum layer surfaces cannot take a close request (layer-shell has
        # no close protocol) — force kill, matching plain hyprctl kill
        # semantics. Layer rects span the whole output (input regions clip
        # what receives clicks), so a hit here may be "desktop background"
        # as much as the visible bar; killing is the honest current behavior.
        action = ["kill", "-KILL", str(target["pid"])]
        what = f"quantum layer surface {target['namespace']} pid {target['pid']}"
    else:
        print(f"qkill: nothing resolvable under the point, doing nothing", file=sys.stderr)
        return 1

    if args.dry_run:
        print(f"qkill: would {what} -> {' '.join(action)}")
        return 0

    subprocess.run(action, check=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
