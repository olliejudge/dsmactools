# Input captures

`dsmactools --record session.jsonl --duration 10` creates a new UTF-8 JSONL file. Files are opened exclusively and never overwritten. Menu recordings default to a timestamped filename in the current directory. Keep captures out of source control unless you deliberately create a public fixture.

Schema 1 has three row kinds:

- `metadata`: tool version, USB transport, product ID, requested duration and measurement units.
- `sample`: monotonic `elapsed_us` measured after the host HID read, the full 64-byte `raw` USB report, and decoded `input`.
- `summary`: received sample count, elapsed time, reports per second, host arrival interval minimum/mean/maximum, raw axis minimum/maximum, buttons seen, `complete` and `cancelled`.

`input.sticks` is left X/Y then right X/Y, normalized from 0–255 to −1…1 with no dead zone. `sticks_raw` preserves those four bytes; `triggers_raw` is left/right, 0–255. D-pad values 0–7 mean north clockwise through northwest; 8 is neutral. Buttons are names, including digital trigger flags independently of their analog values.

Touch contacts contain active state, contact ID and raw X/Y coordinates. `gyro_raw` and `accel_raw` contain signed sensor counts, without calibration or physical-unit conversion. `sensor_timestamp`, `sequence` and `battery_raw` preserve controller fields. Extra Edge controls are not decoded.

Use axis ranges and idle captures to inspect stick drift, and compare button activity and report cadence across tests. Host arrival intervals include OS scheduling and application overhead. They are not physical controller latency, calibrated sensor time, or game frame latency. A controller's sequence and sensor timestamp may wrap.

`complete: true` means no recording error; a user cancellation still yields a valid capture with `cancelled: true`. Signals SIGINT/SIGTERM and terminal q/Esc/Ctrl-C request a graceful stop. Cable loss, storage failure or forced termination may leave a partial file without a summary. A zero-sample recording fails instead of presenting it as a successful test; its axis ranges are not meaningful.
