# Developer and testing tools

As well as firmware updates, the menu includes tools for game developers and controller testing:

| Tool | What it does |
| --- | --- |
| Live input monitor | Sticks, buttons, triggers, touch and raw motion data |
| Session recording | Raw and decoded USB inputs saved as JSONL, with timing and axis summaries |
| Feedback lab | Every RGB lightbar color, rainbow cycling and seven adaptive trigger modes |
| Developer diagnostics | Xcode, Swift, Metal and native macOS GameController capabilities |
| Firmware | Check, download or install official updates |

The menu uses arrow keys and Enter; Esc returns or exits. Live tools stop with q, Esc or Ctrl-C. Feedback tests reset their effects when they finish. Close games and other controller tools before USB tests.

The same tools are available as command-line options, which suit scripts:

```sh
dsmactools --monitor
dsmactools --record session.jsonl --duration 10
dsmactools --feedback rumble --duration 3
dsmactools --diagnostics
dsmactools --check
dsmactools --print-firmware-info --json
dsmactools --help
```

Capture timing measures host report arrival, not game latency. See the [capture format](captures.md) and [lightbar and trigger testing](feedback.md).

Standard DualSense USB operation is hardware-tested. DualSense Edge is detected but not hardware-tested. Direct input, feedback and firmware tools require USB; native macOS diagnostics can also see other supported controllers and Bluetooth devices.
