# Dashboard file format

Dashboards are JSON5 files (JSON with comments, unquoted keys and trailing commas). The
visual editor writes plain JSON, which is valid JSON5.

```json5
{
  name: "My dash",
  canvas: { width: 1600, height: 600 },     // design units; scaled to fit any screen
  theme: { accent: "#ffae3d", danger: "#ff4d3d" /* ink, muted, warn, ok, info, face, edge, unit */ },
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
| `analog_gauge` | `bind`, `min`, `max`, `zones`, `major_ticks`, `minor_ticks`, `start_angle`, `sweep`, `caption`, `indicator: needle/arc`, `hub`, `center: { bind, format, show_unit }` |
| `bar_gauge` | `bind`, `min`, `max`, `zones`, `vertical`, `label`, `show_value`, `format`, `ramp_segments`, `shape: bar/center/curve` |
| `readout` | `bind`, `format: { style: number/time/gear/on_off, decimals (0–6), unit }`, `label`, `label_position: above/below`, `zones`, `align` |
| `gear` | `bind` (default gear), `shift_at`, `frame: none/box/hex` |
| `shift_lights` | `bind` (default rpm), `start`, `limit`, `segments`, `colors`, `flash_color`, `shape: round/bar` |
| `lamp` | `bind`, `icon`, `color`, `above` (default 0.5) or `below`, `blink` |
| `tyre_quad` | `signals` (FL, FR, RL, RR), `zones`, `format`, `label`, `show_unit`, `layout: grid/car` |
| `g_meter` | `max_g` |
| `pedals` | `signals` (1–4), `colors`, `show_labels`, `show_values` |
| `lap_timer` | `show_last`, `show_best`, `layout: stack/columns` |
| `label` | `text` with `{signal}`, `{signal:1}` (0–6 decimals), `{signal:time}`, `{signal:gear}`, `{signal:on_off}` placeholders, `align`, `color` |
| `image` | `src` (relative to the dashboard file), `fit: contain/cover/fill`. Shown on the computer; phones don't show images yet |
| `panel` | Background from `style.face` and `style.edge`; `variant: box/divider/chamfer/strip`, `edges` (for `divider`: any of `top`, `bottom`, `left`, `right`; all four by default), `title` (drawn in `style.muted`). Use `z: -1` to put it behind instruments |

Analog gauges default to a needle. Set `indicator: "arc"` for an illuminated sweep and a larger center value.
Set `hub: true` to frame the center value in a dark disc; with a needle, the needle then starts at the hub's rim.
Set `center.show_unit` to `true` to display the center binding's unit, which follows the active unit settings.
A horizontal bar can use `ramp_segments` from 2 to 64 for a rising segmented strip that follows its `zones`.
The default of `0` draws a continuous bar.
`shape: "center"` fills from zero toward the value, for signals that go both ways (`accel_long`, `steer`); color
the two sides with zones such as `{ to: 0, color: "danger" }, { from: 0, color: "ok" }`. `shape: "curve"` draws
a band that rises and sweeps across, with the range's tick numbers along a red rail, like a data logger's RPM
graph; bind it with `scale: 0.001` to number it in thousands.

Panel variants: `box` is a rounded card, `divider` draws hairlines on the chosen `edges` only, `chamfer` cuts
the corners, and `strip` is a square bar for headers and footers (its `title` sits at the left, vertically
centered).

Files are limited to 1 MB and 64 levels of nesting (real dashboards are a few KB).

Every widget also takes `id`, `x`, `y`, `w`, `h`, `z` (layer) and
`style: { ink, accent, face, edge, muted, danger, unit, font, font_scale }`. Colors are `#rrggbb[aa]` or a
theme name. `muted` colors labels and captions, and `unit` colors unit labels (it defaults to the theme's
`unit`, which defaults to `muted`).

## Fonts

Dashboards use Saira Condensed, a condensed motorsport sans. Set `style.font: "digital"` on a widget to show
its values in DSEG7, a seven-segment LCD face; labels stay in Saira. Both fonts are embedded, so dashboards
look the same on every computer and phone.

## Signals

speed, rpm, rpm_max, rpm_idle, gear, power, torque, boost, throttle, brake, clutch,
handbrake, steer, fuel, coolant_temp, oil_temp, tyre_temp_fl/fr/rl/rr, susp_fl/fr/rl/rr,
slip_fl/fr/rl/rr, accel_lat, accel_long, accel_vert, distance, lap_time, last_lap,
best_lap, lap_number, race_position, race_time, race_on, engine_warning, abs_active,
tc_active, lights, blinker_left, blinker_right, pit_limiter, car_id, car_class, car_pi,
cylinders, pos_x, pos_y, pos_z.

Forza provides most of these; it doesn't send coolant/oil temperature, ABS/TC, lights
or indicators, so widgets bound to those show "no data" (dimmed) with Forza.

## Checking a layout

Render a dashboard to a PNG with extreme values to make sure nothing overflows or overlaps.
`--set` takes source units (m/s, rpm, kPa, W, N·m, °C, m/s², seconds; 0–1 for pedals and fuel):

```sh
dashcluster --dashboards my.json5 -d my.json5 --units imperial snapshot --out check.png \
  --set speed=100 --set rpm=9850 --set rpm_max=10000 --set gear=-1 --set lap_time=599.999
```

Values shrink to fit their widget, but a readout that is too narrow for its longest value still gets small.
The built-in dashboards are tested to keep every instrument inside the canvas, off each other, and fully
on top of any panel it sits on.
