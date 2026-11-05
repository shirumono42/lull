//! Pomodoro timer: focus → break → … → long break, then done.
//!
//! Time is kept as "elapsed in this phase", so changing the durations while
//! the timer runs simply moves the end of the current phase.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Focus,
    Break,
    LongBreak,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plan {
    pub focus: Duration,
    pub short: Duration,
    pub long: Duration,
    pub rounds: u32,
}

impl Plan {
    fn length(&self, p: Phase) -> Duration {
        match p {
            Phase::Focus => self.focus,
            Phase::Break => self.short,
            Phase::LongBreak => self.long,
        }
    }
}

/// What happened when the timer moved on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Started(Phase),
    /// The last long break ended; the timer is idle again.
    Finished,
}

#[derive(Clone, Copy, Debug)]
pub struct Timer {
    pub phase: Phase,
    /// 1-based index of the current focus session.
    pub round: u32,
    active: bool,
    base: Duration,
    since: Option<Instant>,
}

impl Default for Timer {
    fn default() -> Self {
        Timer { phase: Phase::Focus, round: 1, active: false, base: Duration::ZERO, since: None }
    }
}

impl Timer {
    /// Running or paused (not idle).
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_running(&self) -> bool {
        self.since.is_some()
    }

    fn elapsed(&self, now: Instant) -> Duration {
        self.base + self.since.map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    pub fn remaining(&self, plan: &Plan, now: Instant) -> Duration {
        plan.length(self.phase).saturating_sub(self.elapsed(now))
    }

    /// 0..1 through the current phase.
    pub fn progress(&self, plan: &Plan, now: Instant) -> f32 {
        let len = plan.length(self.phase).as_secs_f32().max(1.0);
        (self.elapsed(now).as_secs_f32() / len).min(1.0)
    }

    /// Starts from idle, or resumes when paused.
    pub fn start(&mut self, now: Instant) -> Option<Event> {
        if self.is_running() {
            return None;
        }
        let fresh = !self.active;
        self.active = true;
        self.since = Some(now);
        fresh.then_some(Event::Started(Phase::Focus))
    }

    pub fn pause(&mut self, now: Instant) {
        self.base = self.elapsed(now);
        self.since = None;
    }

    pub fn reset(&mut self) {
        *self = Timer::default();
    }

    /// Ends the current phase now.
    pub fn skip(&mut self, plan: &Plan, now: Instant) -> Option<Event> {
        if !self.active {
            return None;
        }
        Some(self.advance(plan, now, Duration::ZERO))
    }

    /// Moves to the next phase once the current one has run out.
    pub fn tick(&mut self, plan: &Plan, now: Instant) -> Option<Event> {
        if !self.is_running() {
            return None;
        }
        let over = self.elapsed(now).checked_sub(plan.length(self.phase))?;
        Some(self.advance(plan, now, over))
    }

    fn advance(&mut self, plan: &Plan, now: Instant, carry: Duration) -> Event {
        let next = match self.phase {
            Phase::Focus if self.round >= plan.rounds => Phase::LongBreak,
            Phase::Focus => Phase::Break,
            Phase::Break => {
                self.round += 1;
                Phase::Focus
            }
            Phase::LongBreak => {
                self.reset();
                return Event::Finished;
            }
        };
        self.phase = next;
        self.base = carry;
        if self.since.is_some() {
            self.since = Some(now);
        }
        Event::Started(next)
    }

    /// Time until the displayed whole second changes, for scheduling wakeups.
    pub fn next_tick(&self, plan: &Plan, now: Instant) -> Option<Instant> {
        if !self.is_running() {
            return None;
        }
        let rem = self.remaining(plan, now);
        let frac = rem.subsec_nanos();
        let wait = if frac == 0 { Duration::from_secs(1) } else { Duration::from_nanos(frac as u64) };
        Some(now + wait + Duration::from_millis(5))
    }
}

/// `mm:ss` (or `h:mm:ss`), rounding up so a fresh 25 min timer shows 25:00.
pub fn clock(d: Duration) -> String {
    let s = d.as_secs() + (d.subsec_nanos() > 0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        let m = |x: u64| Duration::from_secs(x * 60);
        Plan { focus: m(25), short: m(5), long: m(15), rounds: 2 }
    }

    #[test]
    fn full_cycle() {
        let p = plan();
        let t0 = Instant::now();
        let min = |x: u64| t0 + Duration::from_secs(x * 60);
        let mut t = Timer::default();
        assert_eq!(t.start(t0), Some(Event::Started(Phase::Focus)));
        assert_eq!(t.tick(&p, min(24)), None);
        assert_eq!(t.tick(&p, min(25)), Some(Event::Started(Phase::Break)));
        assert_eq!(t.tick(&p, min(30)), Some(Event::Started(Phase::Focus)));
        assert_eq!(t.round, 2);
        assert_eq!(t.tick(&p, min(55)), Some(Event::Started(Phase::LongBreak)));
        assert_eq!(t.tick(&p, min(70)), Some(Event::Finished));
        assert!(!t.is_active());
    }

    #[test]
    fn pause_freezes_time() {
        let p = plan();
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.start(t0);
        t.pause(t0 + Duration::from_secs(60));
        let later = t0 + Duration::from_secs(3600);
        assert_eq!(t.remaining(&p, later), Duration::from_secs(24 * 60));
        assert_eq!(t.tick(&p, later), None);
        assert_eq!(t.start(later), None); // resume is not a new phase
        assert_eq!(t.remaining(&p, later), Duration::from_secs(24 * 60));
    }

    #[test]
    fn skip_moves_on() {
        let p = plan();
        let t0 = Instant::now();
        let mut t = Timer::default();
        assert_eq!(t.skip(&p, t0), None);
        t.start(t0);
        assert_eq!(t.skip(&p, t0), Some(Event::Started(Phase::Break)));
        assert_eq!(t.remaining(&p, t0), p.short);
    }

    #[test]
    fn clock_format() {
        assert_eq!(clock(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(clock(Duration::from_millis(59_001)), "01:00");
        assert_eq!(clock(Duration::from_secs(0)), "00:00");
        assert_eq!(clock(Duration::from_secs(5400)), "1:30:00");
    }
}
