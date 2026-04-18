#!/usr/bin/env python3
"""Capture raw NLS websocket frames for event field discovery.

This script subscribes to the livetiming websocket feed for one or more
`eventId` values and writes each received frame as NDJSON so it can be
inspected and analyzed later.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
import time
from collections import Counter
from typing import Any

try:
    import websocket  # type: ignore
except ImportError as exc:  # pragma: no cover - runtime dependency guard
    raise SystemExit(
        "Missing dependency: websocket-client\n"
        "Install with: python3 -m pip install websocket-client"
    ) from exc


WS_URL = "wss://livetiming.azurewebsites.net/"
DEFAULT_EVENT_IDS = (20, 50)
DEFAULT_PIDS = (0, 4)


def now_unix_ms() -> int:
    return int(time.time() * 1000)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Capture websocket frames from livetiming.azurewebsites.net as NDJSON"
        )
    )
    parser.add_argument(
        "--url",
        default=WS_URL,
        help=f"Websocket URL (default: {WS_URL})",
    )
    parser.add_argument(
        "--event-id",
        type=int,
        nargs="+",
        default=list(DEFAULT_EVENT_IDS),
        help="One or more event IDs to capture sequentially (default: 20 50)",
    )
    parser.add_argument(
        "--pid",
        type=int,
        nargs="+",
        default=list(DEFAULT_PIDS),
        help="One or more PIDs to subscribe (default: 0 4)",
    )
    parser.add_argument(
        "--seconds",
        type=int,
        default=120,
        help="Capture duration per event ID in seconds (default: 120)",
    )
    parser.add_argument(
        "--output-dir",
        default="docs/data",
        help="Directory for NDJSON output files (default: docs/data)",
    )
    parser.add_argument(
        "--output-prefix",
        default="nls_ws_raw",
        help="Output file prefix (default: nls_ws_raw)",
    )
    parser.add_argument(
        "--read-timeout",
        type=float,
        default=2.0,
        help="Socket read timeout seconds (default: 2.0)",
    )
    parser.add_argument(
        "--max-frames",
        type=int,
        default=0,
        help="Optional hard frame cap per event (0 = unlimited)",
    )
    return parser.parse_args()


def build_subscribe_message(event_id: int, pids: list[int]) -> str:
    payload = {
        "clientLocalTime": now_unix_ms(),
        "eventId": str(event_id),
        "eventPid": pids,
    }
    return json.dumps(payload, separators=(",", ":"))


def decode_frame(raw_frame: Any) -> tuple[str, str]:
    if isinstance(raw_frame, bytes):
        return "binary", raw_frame.decode("utf-8", errors="replace")
    return "text", str(raw_frame)


def capture_event(
    *,
    ws_url: str,
    event_id: int,
    pids: list[int],
    seconds: int,
    out_path: pathlib.Path,
    read_timeout: float,
    max_frames: int,
) -> dict[str, Any]:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    started = now_unix_ms()
    deadline = time.monotonic() + max(0, seconds)
    frame_count = 0
    parse_ok_count = 0
    pid_counts: Counter[str] = Counter()

    ws = websocket.create_connection(
        ws_url,
        timeout=10,
        header=[
            "Origin: https://livetiming.azurewebsites.net",
            "User-Agent: Mozilla/5.0",
        ],
    )
    ws.settimeout(read_timeout)
    ws.send(build_subscribe_message(event_id, pids))

    with out_path.open("w", encoding="utf-8") as fh:
        while time.monotonic() < deadline:
            if max_frames > 0 and frame_count >= max_frames:
                break

            try:
                raw_frame = ws.recv()
            except websocket.WebSocketTimeoutException:
                continue
            except KeyboardInterrupt:
                break

            frame_type, text = decode_frame(raw_frame)
            parsed: Any = None
            pid: str | None = None
            parse_error: str | None = None
            try:
                parsed = json.loads(text)
                parse_ok_count += 1
                if isinstance(parsed, dict):
                    pid_value = parsed.get("PID")
                    if pid_value is not None:
                        pid = str(pid_value)
                        pid_counts[pid] += 1
            except json.JSONDecodeError as exc:
                parse_error = str(exc)

            record = {
                "captured_at_unix_ms": now_unix_ms(),
                "event_id": event_id,
                "subscribed_pids": pids,
                "frame_type": frame_type,
                "pid": pid,
                "raw": text,
                "json": parsed,
                "json_error": parse_error,
            }
            fh.write(json.dumps(record, ensure_ascii=True))
            fh.write("\n")
            frame_count += 1

    ws.close()
    finished = now_unix_ms()
    return {
        "event_id": event_id,
        "file": str(out_path),
        "frames": frame_count,
        "json_frames": parse_ok_count,
        "pid_counts": dict(pid_counts),
        "started_unix_ms": started,
        "finished_unix_ms": finished,
    }


def main() -> int:
    args = parse_args()
    pids = sorted(set(args.pid))
    event_ids = sorted(set(args.event_id))
    output_dir = pathlib.Path(args.output_dir)
    timestamp = time.strftime("%Y%m%d_%H%M%S", time.gmtime())

    all_stats: list[dict[str, Any]] = []
    print(
        f"Capturing from {args.url} for event IDs {event_ids} with PID list {pids}",
        file=sys.stderr,
    )

    for event_id in event_ids:
        output_name = f"{args.output_prefix}_{event_id}_{timestamp}.ndjson"
        out_path = output_dir / output_name
        print(
            f"[{event_id}] capture start for {args.seconds}s -> {out_path}",
            file=sys.stderr,
        )
        try:
            stats = capture_event(
                ws_url=args.url,
                event_id=event_id,
                pids=pids,
                seconds=args.seconds,
                out_path=out_path,
                read_timeout=args.read_timeout,
                max_frames=max(0, args.max_frames),
            )
        except KeyboardInterrupt:
            print(f"[{event_id}] interrupted", file=sys.stderr)
            return 130
        except Exception as exc:  # pragma: no cover - network/runtime path
            print(f"[{event_id}] capture failed: {exc}", file=sys.stderr)
            return 1

        all_stats.append(stats)
        print(
            f"[{event_id}] frames={stats['frames']} json={stats['json_frames']}"
            f" pid_counts={stats['pid_counts']}",
            file=sys.stderr,
        )

    print(json.dumps({"captures": all_stats}, ensure_ascii=True, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
