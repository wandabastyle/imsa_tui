#!/usr/bin/env python3
"""Analyze raw NLS websocket NDJSON captures and build field catalogs."""

from __future__ import annotations

import argparse
import glob
import json
import pathlib
import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from typing import Any


MESSAGE_KEY_RE = re.compile(r"(msg|message|text|comment|note|info|meldung)", re.IGNORECASE)


def infer_type(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, int):
        return "int"
    if isinstance(value, float):
        return "float"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    return type(value).__name__


def sample_value_text(value: Any, max_len: int = 90) -> str:
    if isinstance(value, str):
        text = value.strip().replace("\n", " ")
    else:
        text = json.dumps(value, ensure_ascii=True)
    if len(text) > max_len:
        return text[: max_len - 3] + "..."
    return text


@dataclass
class PathStats:
    count: int = 0
    types: Counter[str] = field(default_factory=Counter)
    samples: list[str] = field(default_factory=list)

    def push(self, value: Any) -> None:
        self.count += 1
        self.types[infer_type(value)] += 1
        if len(self.samples) >= 4:
            return
        sample = sample_value_text(value)
        if sample and sample not in self.samples:
            self.samples.append(sample)


@dataclass
class PidStats:
    frames: int = 0
    top_level_keys: Counter[str] = field(default_factory=Counter)
    paths: dict[str, PathStats] = field(default_factory=lambda: defaultdict(PathStats))
    message_candidates: dict[str, PathStats] = field(default_factory=lambda: defaultdict(PathStats))


def walk_value(path: str, value: Any, stats: PidStats) -> None:
    if path:
        stats.paths[path].push(value)
        leaf_name = path.split(".")[-1]
        if MESSAGE_KEY_RE.search(leaf_name) and isinstance(value, str) and value.strip():
            stats.message_candidates[path].push(value)

    if isinstance(value, dict):
        for key, nested in value.items():
            child_path = f"{path}.{key}" if path else key
            walk_value(child_path, nested, stats)
    elif isinstance(value, list):
        array_path = f"{path}[]" if path else "[]"
        stats.paths[array_path].push(value)
        for item in value:
            walk_value(array_path, item, stats)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Analyze websocket NDJSON captures and emit field catalogs"
    )
    parser.add_argument(
        "inputs",
        nargs="*",
        default=["docs/data/nls_ws_raw_*.ndjson"],
        help="Input file paths or glob patterns (default: docs/data/nls_ws_raw_*.ndjson)",
    )
    parser.add_argument(
        "--markdown-out",
        default="",
        help="Optional markdown output path",
    )
    parser.add_argument(
        "--json-out",
        default="",
        help="Optional JSON summary output path",
    )
    return parser.parse_args()


def resolve_inputs(patterns: list[str]) -> list[pathlib.Path]:
    resolved: list[pathlib.Path] = []
    for pattern in patterns:
        matches = sorted(glob.glob(pattern))
        if matches:
            resolved.extend(pathlib.Path(path) for path in matches)
            continue
        path = pathlib.Path(pattern)
        if path.exists():
            resolved.append(path)
    unique = sorted({p.resolve() for p in resolved})
    return [pathlib.Path(p) for p in unique]


def analyze_files(paths: list[pathlib.Path]) -> tuple[dict[str, dict[str, PidStats]], dict[str, int]]:
    by_event: dict[str, dict[str, PidStats]] = defaultdict(lambda: defaultdict(PidStats))
    file_line_counts: dict[str, int] = {}

    for path in paths:
        lines = 0
        with path.open("r", encoding="utf-8") as fh:
            for raw_line in fh:
                line = raw_line.strip()
                if not line:
                    continue
                lines += 1
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue

                event_id = str(record.get("event_id", "unknown"))
                payload = record.get("json")
                if not isinstance(payload, dict):
                    continue
                pid = str(payload.get("PID", record.get("pid", "unknown")))

                pid_stats = by_event[event_id][pid]
                pid_stats.frames += 1
                for key in payload:
                    pid_stats.top_level_keys[key] += 1
                walk_value("", payload, pid_stats)
        file_line_counts[str(path)] = lines

    return by_event, file_line_counts


def render_markdown(by_event: dict[str, dict[str, PidStats]], file_counts: dict[str, int]) -> str:
    lines: list[str] = []
    lines.append("## WebSocket Field Catalog (Observed)")
    lines.append("")
    lines.append("Generated by `scripts/nls_ws_analyze.py` from raw NDJSON captures.")
    lines.append("")
    lines.append("### Source Files")
    lines.append("")
    if not file_counts:
        lines.append("No input data found.")
    else:
        for path, count in sorted(file_counts.items()):
            lines.append(f"- `{path}` ({count} lines)")
    lines.append("")

    if not by_event:
        lines.append("No JSON payload objects found in capture data.")
        lines.append("")
        return "\n".join(lines)

    for event_id in sorted(by_event):
        lines.append(f"### eventId `{event_id}`")
        lines.append("")
        pid_map = by_event[event_id]
        for pid in sorted(pid_map):
            stats = pid_map[pid]
            lines.append(f"#### PID `{pid}`")
            lines.append("")
            lines.append(f"- Frames: {stats.frames}")
            lines.append("")

            lines.append("Top-level keys:")
            lines.append("")
            lines.append("| Key | Count |")
            lines.append("| --- | ---: |")
            for key, count in stats.top_level_keys.most_common():
                lines.append(f"| `{key}` | {count} |")
            lines.append("")

            lines.append("Observed field paths:")
            lines.append("")
            lines.append("| Path | Types | Count | Sample values |")
            lines.append("| --- | --- | ---: | --- |")
            for path, path_stats in sorted(
                stats.paths.items(), key=lambda item: (-item[1].count, item[0])
            ):
                types = ", ".join(
                    f"{name}({count})" for name, count in path_stats.types.most_common()
                )
                sample_values = "; ".join(path_stats.samples) if path_stats.samples else ""
                lines.append(
                    f"| `{path}` | {types} | {path_stats.count} | {sample_values} |"
                )
            lines.append("")

            lines.append("Candidate message fields:")
            lines.append("")
            if not stats.message_candidates:
                lines.append("- None observed in this data slice.")
            else:
                lines.append("| Path | Count | Sample values |")
                lines.append("| --- | ---: | --- |")
                for path, path_stats in sorted(
                    stats.message_candidates.items(),
                    key=lambda item: (-item[1].count, item[0]),
                ):
                    samples = "; ".join(path_stats.samples)
                    lines.append(f"| `{path}` | {path_stats.count} | {samples} |")
            lines.append("")

    return "\n".join(lines)


def to_json_ready(by_event: dict[str, dict[str, PidStats]], file_counts: dict[str, int]) -> dict[str, Any]:
    out: dict[str, Any] = {
        "source_files": file_counts,
        "events": {},
    }
    for event_id, pid_map in by_event.items():
        event_out: dict[str, Any] = {}
        for pid, stats in pid_map.items():
            event_out[pid] = {
                "frames": stats.frames,
                "top_level_keys": dict(stats.top_level_keys),
                "paths": {
                    path: {
                        "count": path_stats.count,
                        "types": dict(path_stats.types),
                        "samples": path_stats.samples,
                    }
                    for path, path_stats in stats.paths.items()
                },
                "message_candidates": {
                    path: {
                        "count": path_stats.count,
                        "types": dict(path_stats.types),
                        "samples": path_stats.samples,
                    }
                    for path, path_stats in stats.message_candidates.items()
                },
            }
        out["events"][event_id] = event_out
    return out


def write_text(path: str, content: str) -> None:
    destination = pathlib.Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(content, encoding="utf-8")


def main() -> int:
    args = parse_args()
    inputs = resolve_inputs(args.inputs)
    by_event, file_counts = analyze_files(inputs)

    markdown = render_markdown(by_event, file_counts)
    if args.markdown_out:
        write_text(args.markdown_out, markdown + "\n")
    else:
        print(markdown)

    if args.json_out:
        payload = to_json_ready(by_event, file_counts)
        write_text(args.json_out, json.dumps(payload, ensure_ascii=True, indent=2) + "\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
