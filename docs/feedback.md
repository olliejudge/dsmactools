# Lightbar and adaptive trigger testing

Connect a DualSense by USB, run `dsmactools`, and choose **Feedback lab**. Close games and other controller tools first so they do not overwrite the test.

Use arrow keys and Enter to choose settings. Enter accepts a value's displayed default. Esc cancels a setting and returns to the main menu. Tests last **1–30 seconds**; q, Esc or Ctrl-C stops a running test early. The app resets both triggers, stops rumble and releases lightbar control when a test finishes or is cancelled. Cleanup is also attempted after errors; reconnect the controller if an effect remains after a cable disconnect or forced process termination.

## Lightbar

Choose a named color, enter any **24-bit RGB color**, or run a smooth rainbow cycle. Brightness scales each channel from 0 (off) to 255 (full). The named palette contains red, orange, amber, yellow, lime, green, mint, cyan, sky blue, blue, indigo, purple, magenta, pink, white and off. Custom input accepts `#RRGGBB`, `RRGGBB` or three comma-separated channels from 0–255.

The rainbow runs a full cycle every 12 seconds. Colors on a screen are a guide; the controller's LEDs may look different. After a test, the controller resumes its own lightbar behavior rather than preserving the selected color.

```sh
dsmactools --feedback lightbar --color purple --duration 5
dsmactools --feedback lightbar --color '#FF8040' --brightness 128 --duration 5
dsmactools --feedback lightbar --color '30,140,255' --duration 5
dsmactools --feedback lightbar --cycle --duration 12
```

## Adaptive triggers

Choose **left (L2)**, **right (R2)** or **both**. Pull the selected trigger during the test to feel the effect. The live display shows both trigger positions as percentages, so you can compare the selected effect with the other trigger or an off baseline.

Zones run from **0, near the released position, to 9, near a full pull**. Zone positions and force levels are protocol settings, not calibrated physical distances or force measurements. Most modes start with mild strength 2; increase gradually. Frequency values are controller settings and are not measured output rates.

| Mode | Purpose | Settings |
| --- | --- | --- |
| Resistance | Steady resistance from a chosen zone onward | Start 0–9; strength 1–8 |
| Weapon | Resistance between two positions, followed by a release | Start 2–7; end above start, at most 8; strength 1–8 |
| Vibration | Pulsing resistance from a chosen zone onward | Start 0–9; strength 1–8; frequency 1–255 |
| Bow | Draw resistance with a separate snap level | Start 1–7; end above start, at most 8; strength and snap strength 1–8 |
| Galloping | Alternating pulses with two positions within the pattern | Start 0–8; end above start, at most 9; first pulse 0–6; second above first, at most 7; frequency 1–255 |
| Machine | Vibration that changes between two strength levels | Start 1–8; end above start, at most 9; both strengths 0–7; frequency 1–255; period 0–255 |
| Off | Free movement for comparison | No effect parameters |

Frequency and machine period are controller protocol values; the app does not convert them to measured oscillation rates or seconds. Galloping defaults to a slower frequency setting of 4 so its alternating pulses are easier to distinguish.

The advanced resistance and vibration choices accept **ten comma-separated levels**, one per zone. A level of 0 disables that zone; 1–8 sets its force or vibration amplitude. Vibration also takes a frequency. On the command line, `--zones` replaces `--start` and `--strength` for these two modes.

```sh
dsmactools --feedback triggers --trigger left --effect resistance --start 4 --strength 2 --duration 5
dsmactools --feedback triggers --trigger right --effect weapon --start 4 --end 7 --strength 2 --duration 5
dsmactools --feedback triggers --effect vibration --start 4 --strength 2 --frequency 25 --duration 5
dsmactools --feedback triggers --effect bow --start 4 --end 7 --strength 2 --snap-strength 2 --duration 5
dsmactools --feedback triggers --effect galloping --start 4 --end 7 --first-foot 1 --second-foot 3 --frequency 4 --duration 5
dsmactools --feedback triggers --effect machine --start 4 --end 7 --strength 2 --strength-b 4 --frequency 25 --period 20 --duration 5
dsmactools --feedback triggers --effect resistance --zones '0,0,0,0,1,1,2,2,3,3' --duration 5
dsmactools --feedback triggers --effect off --duration 5
```

Invalid settings and options for a different effect are rejected before the controller is opened. These are volatile USB feedback reports, separate from firmware updates. They do not edit or install controller firmware. See [research and hardware evidence](research.md) for validation and device coverage.
