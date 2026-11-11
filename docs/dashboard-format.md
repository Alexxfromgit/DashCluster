# Dashboard file format

Dashboards are JSON5 files (JSON with comments, unquoted keys and trailing commas). The
visual editor writes plain JSON, which is valid JSON5.

```json5
{
  name: "My dash",
  canvas: { width: 1600, height: 600 },     // design units; scaled to fit any screen
  theme: { accent: "#ffae3d", danger: "#ff4d3d" /* ink, muted, warn, ok, info, face, edge */ },
  background: { color: "#04070d", gradient_to: "#0b1426", gradient_angle: 180, image: "bg.png" },
  widgets: [
    { id: "tach", type: "analog_gauge", x: 30, y: 30, w: 540, h: 540,
      bind: { signal: "rpm", scale: 0.001, smoothing: 0.04 },
      max: { signal: "rpm_max", scale: 0.001, ceil_to: 1, fallback: 8 },
      zones: [ { from: { signal: "rpm_max", scale: 0.00092 }, color: "danger" } ] },
  ],
}
```

## Units

Numbers in a layout (ranges, zones, thresholds) are in **metric display units** after the
binding's `scale`: km/h, °C, bar, kW, N·m, g, and **percent** for 0–1 signals
(throttle, fuel, slip…). If the user picks other units, ranges are converted and gauge
ticks re-rounded automatically.

## Bindings

`bind: "speed"` or `bind: { signal, scale, offset, smoothing, ceil_to, convert, fallback }`.
Anywhere a number is expected (`min`, `max`, zone `from`/`to`, `start`, `limit`,
`shift_at`) you can use a binding instead, e.g. `max: { signal: "rpm_max", scale: 0.001 }`.

## Widgets

| type | Key fields |
|---|---|
| `analog_gauge` | `bind`, `min`, `max`, `zones`, `major_ticks`, `minor_ticks`, `start_angle`, `sweep`, `caption`, `center: { bind, format }` |
| `bar_gauge` | `bind`, `min`, `max`, `zones`, `vertical`, `label`, `show_value`, `format` |
| `readout` | `bind`, `format: { style: number/time/gear/on_off, decimals, unit }`, `label`, `zones`, `align` |
| `gear` | `bind` (default gear), `shift_at` |
| `shift_lights` | `bind` (default rpm), `start`, `limit`, `segments`, `colors`, `flash_color` |
| `lamp` | `bind`, `icon`, `color`, `above` (default 0.5) or `below`, `blink` |
| `tyre_quad` | `signals` (FL, FR, RL, RR), `zones`, `format`, `label` |
| `g_meter` | `max_g` |
| `pedals` | `signals` (1–4), `colors`, `show_labels` |
| `lap_timer` | `show_last`, `show_best` |
| `label` | `text` with `{signal}`, `{signal:1}`, `{signal:time}`, `{signal:gear}` placeholders, `align`, `color` |
| `image` | `src` (relative to the dashboard file), `fit: contain/cover/fill` |

Every widget also takes `id`, `x`, `y`, `w`, `h`, `z` (layer) and
`style: { ink, accent, face, muted, danger, font_scale }`. Colors are `#rrggbb[aa]` or a
theme name.

## Signals

speed, rpm, rpm_max, rpm_idle, gear, power, torque, boost, throttle, brake, clutch,
handbrake, steer, fuel, coolant_temp, oil_temp, tyre_temp_fl/fr/rl/rr, susp_fl/fr/rl/rr,
slip_fl/fr/rl/rr, accel_lat, accel_long, accel_vert, distance, lap_time, last_lap,
best_lap, lap_number, race_position, race_time, race_on, engine_warning, abs_active,
tc_active, lights, blinker_left, blinker_right, pit_limiter, car_id, car_class, car_pi,
cylinders, pos_x, pos_y, pos_z.

Forza provides most of these; it doesn't send coolant/oil temperature, ABS/TC, lights
or indicators, so widgets bound to those show "no data" (dimmed) with Forza.
