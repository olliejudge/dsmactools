#!/usr/bin/env python3
"""Report Sony catalogue changes without downloading firmware or touching HID."""
import json
import re
import subprocess
import sys
from pathlib import Path

URL = "https://fwupdater.dl.playstation.net/fwupdater/info.json"

def main():
    baseline = json.loads((Path(__file__).resolve().parents[1] / "catalogue-baseline.json").read_text())
    result = subprocess.run(["curl", "--fail", "--silent", "--show-error", "--proto", "=https",
                             "--connect-timeout", "10", "--max-time", "30", "--max-filesize", "65536", URL],
                            check=True, capture_output=True, timeout=35)
    current = json.loads(result.stdout)
    if not isinstance(current, dict) or not current:
        raise ValueError("Empty or malformed Sony catalogue")
    for key, value in current.items():
        if key.startswith("FwUpdate") and not (re.fullmatch(r"FwUpdate[0-9A-F]{4}LatestVersion", key)
                                               and isinstance(value, str) and re.fullmatch(r"0x[0-9A-Fa-f]{4}", value)):
            raise ValueError(f"Unexpected firmware catalogue entry: {key!r}: {value!r}")
    changes = [{"key": key, "previous": baseline.get(key), "current": current.get(key)}
               for key in sorted(set(baseline) | set(current)) if baseline.get(key) != current.get(key)]
    print(json.dumps({"source": URL, "catalogue": current, "changes": changes}, indent=2))
    return 2 if changes else 0

if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"Catalogue check failed: {error}", file=sys.stderr)
        sys.exit(1)
