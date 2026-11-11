//! A simulated car driving laps of a made-up circuit. Deterministic, so it doubles as a
//! test fixture: the same `dt` sequence always produces the same telemetry.

use cluster_core::{SignalId as S, SignalSet, Snapshot, SourceContext, SourceState, TelemetrySource};
use std::f32::consts::PI;
use std::time::{Duration, Instant};

const GEAR_RATIOS: [f32; 6] = [3.60, 2.19, 1.56, 1.21, 1.00, 0.84];
const FINAL_DRIVE: f32 = 3.73;
const WHEEL_RADIUS: f32 = 0.33;
const MASS: f32 = 1450.0;
const RPM_IDLE: f32 = 850.0;
const RPM_REDLINE: f32 = 7200.0;
const RPM_MAX: f32 = 7600.0;
const DRIVETRAIN_EFF: f32 = 0.88;
const CDA: f32 = 0.68;
const AIR_DENSITY: f32 = 1.2;
const ROLLING: f32 = 180.0;
const MAX_BRAKE_FORCE: f32 = 14_000.0;
const SHIFT_TIME: f32 = 0.18;
const FUEL_CAPACITY_L: f32 = 60.0;

#[derive(Clone, Copy)]
struct Segment {
    length: f32,
    /// 0 = straight; signed curvature 1/R (positive = right-hander).
    curvature: f32,
    target_speed: f32,
}

const fn seg(length: f32, radius: f32, target_kmh: f32) -> Segment {
    Segment { length, curvature: if radius == 0.0 { 0.0 } else { 1.0 / radius }, target_speed: target_kmh / 3.6 }
}

/// A ~3.3 km fictional circuit.
const TRACK: &[Segment] = &[
    seg(620.0, 0.0, 250.0),
    seg(90.0, 45.0, 85.0),
    seg(300.0, 0.0, 200.0),
    seg(140.0, -120.0, 140.0),
    seg(180.0, 0.0, 190.0),
    seg(70.0, 30.0, 70.0),
    seg(60.0, -35.0, 75.0),
    seg(450.0, 0.0, 240.0),
    seg(160.0, 80.0, 120.0),
    seg(260.0, 0.0, 210.0),
    seg(110.0, -55.0, 95.0),
    seg(380.0, 0.0, 230.0),
    seg(200.0, 150.0, 155.0),
    seg(280.0, 0.0, 220.0),
];

fn track_length() -> f32 {
    TRACK.iter().map(|s| s.length).sum()
}

/// Engine torque (N·m) at full throttle.
fn full_torque(rpm: f32) -> f32 {
    const CURVE: [(f32, f32); 7] = [
        (800.0, 210.0),
        (2000.0, 330.0),
        (3500.0, 410.0),
        (4800.0, 430.0),
        (6000.0, 405.0),
        (7200.0, 350.0),
        (7600.0, 300.0),
    ];
    if rpm <= CURVE[0].0 {
        return CURVE[0].1;
    }
    for w in CURVE.windows(2) {
        let ((r0, t0), (r1, t1)) = (w[0], w[1]);
        if rpm <= r1 {
            return t0 + (t1 - t0) * (rpm - r0) / (r1 - r0);
        }
    }
    CURVE[CURVE.len() - 1].1
}

fn approach(current: f32, target: f32, rate_per_s: f32, dt: f32) -> f32 {
    current + (target - current) * (1.0 - (-rate_per_s * dt).exp())
}

/// Simulated car state.
#[derive(Clone)]
pub struct DemoCar {
    pub time: f32,
    pub speed: f32,
    pub rpm: f32,
    /// 1-based forward gear.
    pub gear: usize,
    shift_timer: f32,
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
    pub steer: f32,
    lap_distance: f32,
    pub total_distance: f32,
    pub lap: u32,
    lap_start: f32,
    pub last_lap: Option<f32>,
    pub best_lap: Option<f32>,
    pub coolant: f32,
    pub oil: f32,
    pub tyres: [f32; 4],
    pub fuel_l: f32,
    pub boost_kpa: f32,
    accel_long: f32,
    accel_lat: f32,
    abs: bool,
    tc: bool,
    seq: u64,
}

impl Default for DemoCar {
    fn default() -> Self {
        Self::new()
    }
}

impl DemoCar {
    pub fn new() -> Self {
        DemoCar {
            time: 0.0,
            speed: 0.0,
            rpm: RPM_IDLE,
            gear: 1,
            shift_timer: 0.0,
            throttle: 0.0,
            brake: 0.0,
            clutch: 0.0,
            steer: 0.0,
            lap_distance: 0.0,
            total_distance: 0.0,
            lap: 1,
            lap_start: 0.0,
            last_lap: None,
            best_lap: None,
            coolant: 24.0,
            oil: 22.0,
            tyres: [22.0; 4],
            fuel_l: FUEL_CAPACITY_L * 0.85,
            boost_kpa: 0.0,
            accel_long: 0.0,
            accel_lat: 0.0,
            abs: false,
            tc: false,
            seq: 0,
        }
    }

    fn segment_at(distance: f32) -> (usize, f32) {
        let mut d = distance;
        for (i, s) in TRACK.iter().enumerate() {
            if d < s.length {
                return (i, d);
            }
            d -= s.length;
        }
        (TRACK.len() - 1, TRACK[TRACK.len() - 1].length)
    }

    /// Simple driver: brake for the next slower segment, otherwise chase the current
    /// segment's target speed.
    fn drive(&mut self, dt: f32) {
        let (idx, into) = Self::segment_at(self.lap_distance);
        let cur = TRACK[idx];
        let next = TRACK[(idx + 1) % TRACK.len()];
        let to_next = cur.length - into;
        let decel = 9.0;
        let braking_dist = (self.speed.powi(2) - next.target_speed.powi(2)) / (2.0 * decel);
        let (throttle, brake) = if next.target_speed < self.speed && to_next < braking_dist + 8.0 {
            (0.0, ((braking_dist - to_next + 20.0) / 40.0).clamp(0.35, 1.0))
        } else {
            let err = cur.target_speed - self.speed;
            if err > 0.0 { ((err / 6.0).clamp(0.08, 1.0), 0.0) } else { (0.0, (-err / 15.0).clamp(0.0, 0.5)) }
        };
        self.throttle = approach(self.throttle, throttle, 12.0, dt);
        self.brake = approach(self.brake, brake, 14.0, dt);
        self.steer = approach(self.steer, (cur.curvature * 18.0).clamp(-1.0, 1.0), 6.0, dt);
        self.accel_lat = self.speed.powi(2) * cur.curvature;
    }

    fn wheel_rpm(&self, gear: usize) -> f32 {
        self.speed / (2.0 * PI * WHEEL_RADIUS) * 60.0 * GEAR_RATIOS[gear - 1] * FINAL_DRIVE
    }

    /// Advances the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        self.time += dt;
        self.drive(dt);

        // Gearbox
        if self.shift_timer > 0.0 {
            self.shift_timer -= dt;
        } else if self.rpm > RPM_REDLINE * 0.94 && self.gear < GEAR_RATIOS.len() && self.throttle > 0.3 {
            self.gear += 1;
            self.shift_timer = SHIFT_TIME;
        } else if self.gear > 1 && self.wheel_rpm(self.gear - 1) < RPM_REDLINE * 0.78 && self.rpm < 3200.0 {
            self.gear -= 1;
            self.shift_timer = SHIFT_TIME;
        }
        let shifting = self.shift_timer > 0.0;
        self.clutch = approach(self.clutch, if shifting { 1.0 } else { 0.0 }, 30.0, dt);
        let effective_throttle = if shifting { 0.0 } else { self.throttle };

        // Engine speed: locked to the wheels, except a slipping clutch at launch.
        let locked = self.wheel_rpm(self.gear);
        let launch = RPM_IDLE + effective_throttle * 3200.0;
        let target_rpm = if self.gear == 1 && locked < launch { launch } else { locked.max(RPM_IDLE) };
        self.rpm = approach(self.rpm, target_rpm.min(RPM_MAX), if shifting { 18.0 } else { 25.0 }, dt);
        let limiter = self.rpm >= RPM_MAX - 20.0;

        // Longitudinal dynamics
        let torque = full_torque(self.rpm) * effective_throttle * if limiter { 0.0 } else { 1.0 };
        let mut drive = torque * GEAR_RATIOS[self.gear - 1] * FINAL_DRIVE * DRIVETRAIN_EFF / WHEEL_RADIUS;
        let grip_limit = MASS * 9.81 * 1.1;
        self.tc = drive > grip_limit && self.gear <= 2;
        if self.tc {
            drive = grip_limit;
        }
        let drag = 0.5 * AIR_DENSITY * CDA * self.speed.powi(2) + if self.speed > 0.1 { ROLLING } else { 0.0 };
        let braking = self.brake * MAX_BRAKE_FORCE * if self.speed > 0.1 { 1.0 } else { 0.0 };
        self.abs = self.brake > 0.85 && self.speed > 8.0;
        let accel = (drive - drag - braking) / MASS;
        self.speed = (self.speed + accel * dt).max(0.0);
        self.accel_long = approach(self.accel_long, accel, 10.0, dt);

        // Distance and lap timing
        let ds = self.speed * dt;
        self.total_distance += ds;
        self.lap_distance += ds;
        let len = track_length();
        if self.lap_distance >= len {
            self.lap_distance -= len;
            let lap_time = self.time - self.lap_start;
            self.last_lap = Some(lap_time);
            self.best_lap = Some(self.best_lap.map_or(lap_time, |b| b.min(lap_time)));
            self.lap += 1;
            self.lap_start = self.time;
        }

        // Thermal and consumables
        let load = effective_throttle * self.rpm / RPM_REDLINE;
        self.coolant = approach(self.coolant, 88.0 + load * 9.0, 1.0 / 45.0, dt);
        self.oil = approach(self.oil, 95.0 + load * 18.0, 1.0 / 70.0, dt);
        let lat_heat = self.accel_lat.abs() / 9.81;
        for (i, t) in self.tyres.iter_mut().enumerate() {
            let side = if i % 2 == 0 { -1.0 } else { 1.0 }; // left / right
            let outer = (side * self.accel_lat.signum()).max(0.0);
            let target = 55.0 + self.speed * 0.25 + lat_heat * 30.0 * (0.5 + outer) + self.brake * 15.0;
            *t = approach(*t, target, 1.0 / 20.0, dt);
        }
        let power_kw = torque * self.rpm * 2.0 * PI / 60.0 / 1000.0;
        self.fuel_l = (self.fuel_l - power_kw * 0.000_08 * dt - 0.0005 * dt).max(0.0);
        let boost_target =
            if self.rpm > 2200.0 { effective_throttle * ((self.rpm - 2200.0) / 2000.0).min(1.0) * 105.0 } else { 0.0 };
        self.boost_kpa = approach(self.boost_kpa, boost_target, 4.0, dt);
        self.seq += 1;
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut s = Snapshot::new();
        s.seq = self.seq;
        let shifting = self.shift_timer > 0.0;
        let torque = if shifting { 0.0 } else { full_torque(self.rpm) * self.throttle };
        s.set(S::Speed, self.speed);
        s.set(S::Rpm, self.rpm);
        s.set(S::RpmMax, RPM_MAX);
        s.set(S::RpmIdle, RPM_IDLE);
        s.set(S::Gear, self.gear as f32);
        s.set(S::Power, torque * self.rpm * 2.0 * PI / 60.0);
        s.set(S::Torque, torque);
        s.set(S::Boost, self.boost_kpa);
        s.set(S::Throttle, self.throttle);
        s.set(S::Brake, self.brake);
        s.set(S::Clutch, self.clutch);
        s.set(S::Handbrake, 0.0);
        s.set(S::Steer, self.steer);
        s.set(S::Fuel, self.fuel_l / FUEL_CAPACITY_L);
        s.set(S::CoolantTemp, self.coolant);
        s.set(S::OilTemp, self.oil);
        for (id, t) in [S::TyreTempFl, S::TyreTempFr, S::TyreTempRl, S::TyreTempRr].into_iter().zip(self.tyres) {
            s.set(id, t);
        }
        let roll = (self.accel_lat / 9.81 * 0.15).clamp(-0.3, 0.3);
        let pitch = (self.accel_long / 9.81 * 0.12).clamp(-0.3, 0.3);
        s.set(S::SuspFl, 0.5 - roll + pitch);
        s.set(S::SuspFr, 0.5 + roll + pitch);
        s.set(S::SuspRl, 0.5 - roll - pitch);
        s.set(S::SuspRr, 0.5 + roll - pitch);
        let slip = if self.tc {
            0.25
        } else if self.abs {
            0.15
        } else {
            0.02
        };
        for id in [S::SlipFl, S::SlipFr, S::SlipRl, S::SlipRr] {
            s.set(id, slip);
        }
        s.set(S::AccelLat, self.accel_lat);
        s.set(S::AccelLong, self.accel_long);
        s.set(S::AccelVert, 0.0);
        s.set(S::Distance, self.total_distance);
        s.set(S::LapTime, self.time - self.lap_start);
        s.set_opt(S::LastLap, self.last_lap);
        s.set_opt(S::BestLap, self.best_lap);
        s.set(S::LapNumber, self.lap as f32);
        s.set(S::RacePosition, 1.0);
        s.set(S::RaceTime, self.time);
        s.set_flag(S::RaceOn, true);
        s.set_flag(S::EngineWarning, self.coolant > 110.0);
        s.set_flag(S::AbsActive, self.abs);
        s.set_flag(S::TcActive, self.tc);
        s.set_flag(S::Lights, true);
        // Indicate briefly before each corner so warning lamps have something to show.
        let (idx, into) = Self::segment_at(self.lap_distance);
        let next = TRACK[(idx + 1) % TRACK.len()];
        let blink = (self.time * 2.5).fract() < 0.5;
        let before_corner = TRACK[idx].curvature == 0.0 && TRACK[idx].length - into < 120.0;
        s.set_flag(S::BlinkerLeft, before_corner && next.curvature < 0.0 && blink);
        s.set_flag(S::BlinkerRight, before_corner && next.curvature > 0.0 && blink);
        s.set_flag(S::PitLimiter, false);
        s.set(S::Cylinders, 6.0);
        s.set(S::CarId, 0.0);
        s
    }
}

/// The demo car as a [`TelemetrySource`] running at `rate_hz`.
pub struct DemoSource {
    pub rate_hz: f32,
    car: DemoCar,
}

impl DemoSource {
    pub fn new(rate_hz: f32) -> Self {
        DemoSource { rate_hz: rate_hz.clamp(1.0, 240.0), car: DemoCar::new() }
    }
}

impl Default for DemoSource {
    fn default() -> Self {
        Self::new(60.0)
    }
}

impl TelemetrySource for DemoSource {
    fn name(&self) -> String {
        "Demo car".into()
    }

    fn capabilities(&self) -> SignalSet {
        DemoCar::new().snapshot().present().iter().chain([S::LastLap, S::BestLap]).fold(
            SignalSet::EMPTY,
            |mut acc, id| {
                acc.insert(id);
                acc
            },
        )
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        ctx.set_state(SourceState::Live, format!("Simulated at {:.0} Hz", self.rate_hz));
        let period = Duration::from_secs_f32(1.0 / self.rate_hz);
        let mut last = Instant::now();
        while !ctx.should_stop() {
            let now = Instant::now();
            self.car.step((now - last).as_secs_f32());
            last = now;
            ctx.publish(self.car.snapshot());
            std::thread::sleep(period.saturating_sub(now.elapsed()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulate(seconds: f32) -> Vec<Snapshot> {
        let mut car = DemoCar::new();
        let dt = 1.0 / 60.0;
        (0..(seconds / dt) as usize)
            .map(|_| {
                car.step(dt);
                car.snapshot()
            })
            .collect()
    }

    #[test]
    fn drives_laps_within_limits() {
        let frames = simulate(240.0);
        let max_speed = frames.iter().filter_map(|f| f.get(S::Speed)).fold(0.0, f32::max);
        let max_gear = frames.iter().filter_map(|f| f.get(S::Gear)).fold(0.0, f32::max);
        let max_rpm = frames.iter().filter_map(|f| f.get(S::Rpm)).fold(0.0, f32::max);
        let last = frames.last().unwrap();
        assert!(max_speed * 3.6 > 180.0, "top speed {} km/h", max_speed * 3.6);
        assert!(max_gear >= 5.0, "max gear {max_gear}");
        assert!(max_rpm <= RPM_MAX + 1.0, "rpm {max_rpm}");
        assert!(last.get(S::LapNumber).unwrap() >= 2.0, "should complete a lap in 4 minutes");
        let best = last.get(S::BestLap).unwrap();
        assert!((60.0..200.0).contains(&best), "lap time {best}");
        assert!(last.get(S::CoolantTemp).unwrap() > 60.0, "engine warms up");
        assert!(frames.iter().any(|f| f.get(S::Brake).unwrap() > 0.5), "brakes for corners");
        assert!(
            frames.iter().any(|f| f.get(S::Gear).unwrap() < max_gear && f.get(S::Speed).unwrap() > 15.0),
            "downshifts"
        );
    }

    #[test]
    fn deterministic() {
        let a = simulate(30.0);
        let b = simulate(30.0);
        assert_eq!(a.last(), b.last());
    }
}
